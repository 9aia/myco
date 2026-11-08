//! PTY integration tests — `myco-tui`'s `run()` needs a real
//! terminal (raw mode, alternate screen, crossterm events), so these
//! spawn the actual binary under a pseudo-terminal and drive it with
//! keystrokes. They cover the event loop, editor suspend/resume,
//! session save, and the terminal-setup error arms; `main.rs` rides
//! along as the process entry point.

#![allow(clippy::unwrap_used)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

/// A temp repo: `project/` worktree with one task, plus throwaway
/// `MYCO_HOME`/`MYCO_CONFIG` so the user's real config can't leak in.
struct Fixture {
    _dir: tempfile::TempDir,
    _home: tempfile::TempDir,
    _cfg: tempfile::TempDir,
    cwd: PathBuf,
    home: PathBuf,
    config: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let wt = dir.path().join("project");
    std::fs::create_dir_all(&wt).unwrap();
    let store =
        myco_store_md::MdStore::new(camino::Utf8PathBuf::from_path_buf(wt.clone()).unwrap());
    let mut ledger = myco_ops::items::load_ledger(&store).unwrap();
    ledger.put_todo(
        myco_core::Item::new(myco_core::ItemId::new(myco_core::ItemKind::Task, 1), "t1"),
        None,
    );
    myco_ops::items::save_ledger(&store, &ledger).unwrap();

    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let config = cfg.path().join("myco.toml");
    std::fs::write(&config, "").unwrap();
    Fixture {
        cwd: dir.path().to_path_buf(),
        home: home.path().to_path_buf(),
        config,
        _dir: dir,
        _home: home,
        _cfg: cfg,
    }
}

fn command(f: &Fixture) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_myco-tui"));
    cmd.cwd(&f.cwd);
    cmd.env("MYCO_HOME", &f.home);
    cmd.env("MYCO_CONFIG", &f.config);
    cmd
}

/// Spawn under a PTY; returns (master, child). The slave is closed
/// in the parent — the child holds its own copies.
fn spawn_pty(cmd: CommandBuilder) -> (Box<dyn MasterPty + Send>, Box<dyn Child + Send + Sync>) {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    (pair.master, child)
}

/// Type `keys` into the pty.
fn send(master: &mut Box<dyn MasterPty + Send>, keys: &str) {
    master
        .take_writer()
        .unwrap()
        .write_all(keys.as_bytes())
        .unwrap();
}

/// Wait for exit, bounded — a hung TUI fails the test instead of
/// hanging the suite.
fn wait_exit(child: &mut Box<dyn Child + Send + Sync>) -> portable_pty::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < deadline, "myco-tui did not exit");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A pty session with a drainer thread: output accumulates in a
/// shared buffer and Device Status Report queries (`ESC[6n`, sent
/// when the TUI resumes after `$EDITOR`) get an automatic cursor
/// position reply — a real terminal answers these, a bare pty
/// doesn't.
struct Drained {
    input: Arc<Mutex<Box<dyn Write + Send>>>,
    output: Arc<Mutex<Vec<u8>>>,
}

fn drain(master: &mut Box<dyn MasterPty + Send>) -> Drained {
    let mut reader = master.try_clone_reader().unwrap();
    let writer = master.take_writer().unwrap();
    let d = Drained {
        input: Arc::new(Mutex::new(writer)),
        output: Arc::new(Mutex::new(Vec::new())),
    };
    let (input, output) = (d.input.clone(), d.output.clone());
    std::thread::spawn(move || {
        let mut buf = [0u8; 512];
        let mut tail: Vec<u8> = Vec::new();
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    output.lock().unwrap().extend_from_slice(&buf[..n]);
                    tail.extend_from_slice(&buf[..n]);
                    if tail.len() > 16 {
                        tail.drain(..tail.len() - 16);
                    }
                    if tail.ends_with(b"\x1b[6n") {
                        let _ = input.lock().unwrap().write_all(b"\x1b[1;1R");
                        tail.clear();
                    }
                }
            }
        }
    });
    d
}

impl Drained {
    fn send(&self, keys: &str) {
        self.input
            .lock()
            .unwrap()
            .write_all(keys.as_bytes())
            .unwrap();
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned()
    }

    /// Block until `needle` appears in the output (bounded).
    fn wait_for(&self, needle: &[u8]) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if self
                .output
                .lock()
                .unwrap()
                .windows(needle.len())
                .any(|w| w == needle)
            {
                return;
            }
            assert!(Instant::now() < deadline, "never saw {needle:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn quits_cleanly_and_saves_session() {
    let f = fixture();
    let (mut master, mut child) = spawn_pty(command(&f));
    let d = drain(&mut master);
    // first frame ⇒ the loop and its signal handlers are installed
    d.wait_for(b"TODO");
    // SGR mouse events (press/release/wheel), a real resize
    // (SIGWINCH → Event::Resize), and a bracketed paste (→ the
    // loop's catch-all arm)
    master
        .resize(PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    d.send("\x1b[<0;10;5M\x1b[<0;10;5m\x1b[<64;10;5M\x1b[<65;10;5M\x1b[I\x1b[O\x1b[200~pasted\x1b[201~");
    std::thread::sleep(Duration::from_millis(800)); // ≥3 poll cycles
    d.send("q");
    let status = wait_exit(&mut child);
    assert!(
        status.success(),
        "exit {:?}: {}",
        status.exit_code(),
        d.text()
    );
    assert!(d.text().contains("TODO"), "no board drawn");
    // `q` saves the session — the next launch would restore it
    let sessions = std::fs::read_dir(f.home.join("ui")).unwrap();
    assert!(sessions.count() > 0, "no session file written");
}

#[test]
fn mid_run_pty_death_surfaces_an_error() {
    let f = fixture();
    // no drain() here — its cloned fds would keep the master alive;
    // we need every master-side fd gone for the child to see EIO
    let (master, mut child) = spawn_pty(command(&f));
    // let the first frame draw — the loop is spinning on poll/read
    std::thread::sleep(Duration::from_millis(800));
    drop(master);
    let status = wait_exit(&mut child);
    assert!(!status.success());
}

#[test]
fn editor_suspend_resume_and_reload() {
    let f = fixture();
    let mut cmd = command(&f);
    // `E` opens $EDITOR on TODO.md — `/bin/true` exits instantly,
    // exercising suspend → editor → resume → reload
    cmd.env("EDITOR", "/bin/true");
    let (mut master, mut child) = spawn_pty(cmd);
    let d = drain(&mut master);
    d.send("E");
    // the cursor-position query only fires on editor resume — once
    // it's answered and the frame redraws, q can't race the reply
    d.wait_for(b"\x1b[6n");
    std::thread::sleep(Duration::from_millis(300));
    d.send("q");
    let status = wait_exit(&mut child);
    assert!(
        status.success(),
        "exit {:?}: {}",
        status.exit_code(),
        d.text()
    );
}

#[test]
fn editor_failure_exits_with_error() {
    let f = fixture();
    let mut cmd = command(&f);
    cmd.env("EDITOR", "/nonexistent-myco-editor-xyz");
    let (mut master, mut child) = spawn_pty(cmd);
    send(&mut master, "E");
    let status = wait_exit(&mut child);
    assert!(!status.success());
}

#[test]
fn closed_pty_surfaces_an_error() {
    let f = fixture();
    let (master, mut child) = spawn_pty(command(&f));
    // master dropped → the child's reads/writes hit EIO; run()
    // must return Err rather than spin
    drop(master);
    let status = wait_exit(&mut child);
    assert!(!status.success());
}

#[test]
fn editor_resume_after_pty_death_errors() {
    let f = fixture();
    // an editor that hangs briefly — we kill the pty while it's
    // suspended, so the resume path's terminal ops fail through `?`.
    // No drain() here: its cloned fds would keep the master alive.
    let editor = f.cwd.join("hang-editor.sh");
    std::fs::write(&editor, "#!/bin/sh\nsleep 3\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut cmd = command(&f);
    cmd.env("EDITOR", editor.to_str().unwrap());
    let (mut master, mut child) = spawn_pty(cmd);
    std::thread::sleep(Duration::from_millis(800)); // first frame up
    send(&mut master, "E"); // suspend → editor sleeps
    std::thread::sleep(Duration::from_millis(500));
    drop(master);
    let status = wait_exit(&mut child);
    assert!(!status.success());
}

#[test]
fn editor_unset_defaults_to_vi_and_fails() {
    let f = fixture();
    let mut cmd = command(&f);
    // no $EDITOR anywhere → the `unwrap_or_else` "vi" fallback;
    // vi isn't installed here, so the spawn fails fast
    cmd.env_remove("EDITOR");
    let (mut master, mut child) = spawn_pty(cmd);
    std::thread::sleep(Duration::from_millis(800));
    send(&mut master, "E");
    let status = wait_exit(&mut child);
    assert!(!status.success());
}

#[test]
fn no_project_exits_with_error() {
    let f = fixture();
    // an empty cwd — discovery finds no `project/` worktree, so
    // `Model::open` fails and `run()` returns before touching the
    // terminal (covers the `?` on the open call)
    let empty = tempfile::tempdir().unwrap();
    let mut cmd = command(&f);
    cmd.cwd(empty.path());
    let (_master, mut child) = spawn_pty(cmd);
    let status = wait_exit(&mut child);
    assert!(!status.success());
}

#[test]
fn no_tty_fails_fast() {
    let f = fixture();
    // piped stdio, no controlling terminal for the child — raw
    // mode or the size query must fail, not hang
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_myco-tui"))
        .current_dir(&f.cwd)
        .env("MYCO_HOME", &f.home)
        .env("MYCO_CONFIG", &f.config)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        assert!(Instant::now() < deadline, "myco-tui hung without a tty");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(!status.success());
}
