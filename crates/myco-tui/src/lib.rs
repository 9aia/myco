//! myco-tui — interactive board TUI for a repo's `project/`
//! worktree, or for the cross-repo index universe when `--view`
//! names a `source = "index"` spec (`org-*`).
//!
//! Index boards: the crate stays index-impl free — the caller
//! injects an [`OrgLoader`] that yields each registered repo's
//! slice (`myco ui` wires the redb `load_fresh`). Rows render
//! `repo:ID`; a task verb on a row opens *that* repo's store
//! under the same lock→mutate→commit discipline.
//!
//! A thin front-end over `myco-ops`: every mutation goes through the
//! same ledger ops the CLI verbs use, so writes and commits are
//! identical to `myco todo done` et al. Ported from pm's Bubble Tea
//! reference (`9aia/pm`, `docs/tui.md` there) — the keymap is
//! vim-style: `j/k` move, `h/l`/`tab`/`shift-tab`/`1`/`2`/`3` boards
//! (tab cycles, wrapping), `Space`/`dd` done
//! and `u` undo (both ask `y`/`n` first), `D` defer, `p` promote,
//! `Alt+↑/↓` reorder, `a` add, `/` filter,
//! `:` command line (`:q` `:w` `:e` `:<id>` `:set id`), `Enter`/`→`/
//! double-click open the preview (`j`/`k` scroll, `←`/`esc` back —
//! `←` pops a drill-down stack of the boards/items you opened
//! through), `E` $EDITOR, `R` reload, `?` help,
//! `q`/`Ctrl-C` quit — plus mouse:
//! the wheel scrolls, a click selects an item row or a board tab,
//! a double-click opens the row.
//! The board also auto-reloads: an OS filesystem watcher (inotify /
//! kqueue / FSEvents via `notify`) covers the `project/` worktree in
//! repo mode — or the index data dir + each repo's worktree on
//! index boards — and wakes a content fingerprint scan on change,
//! with a slow periodic scan as fallback. Writes from other `myco`
//! invocations, `git pull`, or hand-edits show up without a
//! keypress.

mod app;
mod render;

pub use app::{OrgLoader, OrgRepo};

use std::io::{self, Stdout};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

/// Restore the terminal on any exit path.
struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
    }
}

/// Panic hook — restore the terminal *before* the default hook
/// prints, otherwise the panic message is erased by the alt-screen
/// exit and the shell is left in raw mode.
fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
        prev(info);
    }));
}

/// Launch the TUI. `dir`/`root` are the same discovery overrides the
/// CLI takes; `no_commit` suppresses the per-mutation commit. `view`
/// names a declarative view (builtin preset or `[views.<name>]`
/// config entry) whose filter narrows every board's item source; a
/// `source = "index"` view swaps the repo store for `org`'s
/// cross-repo universe (required then — `myco ui` supplies it).
pub fn run(
    dir: Option<&Path>,
    root: Option<&Path>,
    no_commit: bool,
    view: Option<String>,
    org: Option<OrgLoader>,
) -> Result<(), String> {
    let mut model = app::Model::open(dir, root, no_commit, view, org)?;
    enable_raw_mode().map_err(|e| format!("raw mode: {e}"))?;
    // Guard first — raw mode is on from here, so every fallible
    // step below (alt screen, terminal init) must restore on `?`.
    let _guard = Guard;
    install_panic_hook();
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .map_err(|e| format!("alt screen: {e}"))?;
    let mut terminal =
        Terminal::new(CrosstermBackend::new(stdout)).map_err(|e| format!("terminal: {e}"))?;

    loop {
        if let Some(path) = model.pending_editor.take() {
            run_editor(&mut terminal, &path)?;
            model.reload();
        }
        terminal
            .draw(|f| render::draw(f, &mut model))
            .map_err(|e| format!("draw: {e}"))?;
        if event::poll(Duration::from_millis(250)).map_err(|e| e.to_string())? {
            match event::read().map_err(|e| e.to_string())? {
                Event::Key(k) if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                    if model.key(k) {
                        model.save_session();
                        break;
                    }
                }
                Event::Mouse(m) => model.mouse(m),
                Event::Resize(..) => {}
                _ => {}
            }
        }
        // auto-refresh — pick up writes that landed while we were
        // idle (other myco invocations, git pull, hand-edits)
        model.tick();
    }
    Ok(())
}

/// Suspend the TUI, run `$EDITOR path`, resume.
fn run_editor(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    path: &Path,
) -> Result<(), String> {
    disable_raw_mode().map_err(|e| e.to_string())?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )
    .map_err(|e| e.to_string())?;
    // `$EDITOR` may carry args (`code --wait`, `vim -p`) — split
    // on whitespace rather than spawning the whole string as one
    // program name
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or("vi");
    let status = Command::new(program).args(parts).arg(path).status();
    enable_raw_mode().map_err(|e| e.to_string())?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )
    .map_err(|e| e.to_string())?;
    terminal.clear().map_err(|e| e.to_string())?;
    status.map_err(|e| format!("$EDITOR {editor}: {e}"))?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// The hook body itself — a caught panic still runs it, which
    /// exercises the terminal-restore path (a no-op without a tty).
    #[test]
    fn panic_hook_restores_the_terminal() {
        install_panic_hook();
        let r = std::panic::catch_unwind(|| panic!("boom"));
        assert!(r.is_err());
    }
}
