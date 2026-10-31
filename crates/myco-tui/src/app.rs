//! The TUI model — boards, cursor, modes, and the ledger ops every
//! key maps to. Mirrors pm's design: two board tabs over
//! `myco_ops::items::Ledger`, a flattened row list, and writes that
//! end in `items::commit`.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use myco_core::{Item, ItemId, ItemKind, Status};
use myco_ops::items::Ledger;
use myco_ops::{discover, items};
use myco_store::Store;
use myco_store_md::MdStore;

/// Which ledger doc the board tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Board {
    /// `TODO.md` — committed work.
    Todo,
    /// `BACKLOG.md` — deferred items.
    Backlog,
}

impl Board {
    /// Tab label.
    pub fn label(self) -> &'static str {
        match self {
            Board::Todo => "TODO",
            Board::Backlog => "BACKLOG",
        }
    }

    /// Tab order for `h`/`l` and the `1`/`2` jumps.
    pub const ALL: [Board; 2] = [Board::Todo, Board::Backlog];
}

/// UI mode — like pm's: the board handles keys differently per mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Browsing.
    Normal,
    /// Full-pane item preview (`Enter`, `esc` back).
    Preview,
    /// `a` — typing a new item title; `Enter` commits, `esc` drops.
    Add,
}

/// A flattened display row — section headers aren't selectable.
#[derive(Debug, Clone)]
pub enum Row {
    /// `## <section>` header line.
    Header(String),
    /// An item line; `depth` indents children under their parent.
    Item { item: Box<Item>, depth: usize },
}

/// The whole TUI state.
pub struct Model {
    proj: discover::Project,
    store: MdStore,
    ledger: Ledger,
    no_commit: bool,
    /// Active tab.
    pub board: Board,
    /// Display rows for `board`, rebuilt on reload/switch.
    pub rows: Vec<Row>,
    /// Index into `rows` (always on an `Item` row when any exist).
    pub sel: usize,
    /// Current mode.
    pub mode: Mode,
    /// `a` input buffer.
    pub input: String,
    /// Status-bar message (last action / error).
    pub status: String,
    /// Editor to open on the next loop turn (`E`).
    pub pending_editor: Option<PathBuf>,
    /// Multi-key prefix: `g` armed for `gg`, `d` for `dd`.
    pending: Option<char>,
}

impl Model {
    /// Discovery + first ledger load. `dir`/`root` override like the
    /// CLI globals.
    pub fn open(dir: Option<&Path>, root: Option<&Path>, no_commit: bool) -> Result<Self, String> {
        let (proj, store) = items::open(dir, root)?;
        let ledger = Ledger::load(&store)?;
        let mut m = Model {
            proj,
            store,
            ledger,
            no_commit,
            board: Board::Todo,
            rows: Vec::new(),
            sel: 0,
            mode: Mode::Normal,
            input: String::new(),
            status: String::new(),
            pending_editor: None,
            pending: None,
        };
        m.rebuild_rows();
        Ok(m)
    }

    /// Reload both docs from disk (`R`, post-editor).
    pub fn reload(&mut self) {
        let sel_id = self.selected_id();
        match Ledger::load(&self.store) {
            Ok(ledger) => {
                self.ledger = ledger;
                self.rebuild_rows();
                self.reselect(sel_id);
            }
            Err(e) => self.status = format!("reload: {e}"),
        }
    }

    /// Flatten the active board's sections into rows.
    fn rebuild_rows(&mut self) {
        let sections = match self.board {
            Board::Todo => &self.ledger.todo.sections,
            Board::Backlog => &self.ledger.backlog.sections,
        };
        self.rows.clear();
        for s in sections {
            if let Some(name) = &s.name {
                self.rows.push(Row::Header(name.clone()));
            }
            push_items(&s.items, 0, &mut self.rows);
        }
        self.sel = self.first_item_row().unwrap_or(0);
    }

    /// After a mutation: land the cursor back on `id` (or the
    /// nearest item row).
    fn reselect(&mut self, id: Option<ItemId>) {
        if let Some(id) = id {
            if let Some(i) = self
                .rows
                .iter()
                .position(|r| matches!(r, Row::Item { item, .. } if item.id == id))
            {
                self.sel = i;
                return;
            }
        }
        self.sel = self
            .first_item_row()
            .unwrap_or(0)
            .min(self.rows.len().saturating_sub(1));
    }

    fn first_item_row(&self) -> Option<usize> {
        self.rows.iter().position(|r| matches!(r, Row::Item { .. }))
    }

    /// The id under the cursor.
    fn selected_id(&self) -> Option<ItemId> {
        self.selected().map(|i| i.id)
    }

    /// The item under the cursor.
    pub fn selected(&self) -> Option<&Item> {
        match self.rows.get(self.sel) {
            Some(Row::Item { item, .. }) => Some(item),
            _ => None,
        }
    }

    /// The board file for `E`.
    fn board_path(&self) -> PathBuf {
        match self.board {
            Board::Todo => self.proj.worktree.join("TODO.md"),
            Board::Backlog => self.proj.worktree.join("BACKLOG.md"),
        }
    }

    /// Handle one key. Returns `true` to quit.
    pub fn key(&mut self, k: KeyEvent) -> Result<bool, String> {
        match self.mode {
            Mode::Preview => return self.key_preview(k),
            Mode::Add => return self.key_add(k),
            Mode::Normal => {}
        }
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let was_g = self.pending == Some('g');
        let was_d = self.pending == Some('d');
        self.pending = None;
        match k.code {
            KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('1') => self.switch_board(0),
            KeyCode::Char('2') => self.switch_board(1),
            KeyCode::Char('h') | KeyCode::Left => self.step_board(-1),
            KeyCode::Char('l') | KeyCode::Right => self.step_board(1),
            KeyCode::Char('j') | KeyCode::Down if alt => self.reorder(1),
            KeyCode::Char('k') | KeyCode::Up if alt => self.reorder(-1),
            KeyCode::Char('j') | KeyCode::Down => self.move_sel(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_sel(-1),
            KeyCode::Char('g') if was_g => self.sel = self.first_item_row().unwrap_or(0),
            KeyCode::Char('g') => self.pending = Some('g'),
            KeyCode::Char('G') => self.sel = self.last_item_row().unwrap_or(0),
            KeyCode::Char('d') if was_d => self.done_selected()?,
            KeyCode::Char('d') => self.pending = Some('d'),
            KeyCode::Char('D') => self.defer_selected()?,
            KeyCode::Char('p') => self.promote_selected()?,
            KeyCode::Char(' ') => self.done_selected()?,
            KeyCode::Char('a') => {
                self.mode = Mode::Add;
                self.input.clear();
            }
            KeyCode::Enter => {
                if self.selected().is_some() {
                    self.mode = Mode::Preview;
                }
            }
            KeyCode::Char('E') => self.pending_editor = Some(self.board_path()),
            KeyCode::Char('R') => self.reload(),
            _ => {}
        }
        Ok(false)
    }

    fn key_preview(&mut self, k: KeyEvent) -> Result<bool, String> {
        match k.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => self.mode = Mode::Normal,
            _ => {}
        }
        Ok(false)
    }

    fn key_add(&mut self, k: KeyEvent) -> Result<bool, String> {
        match k.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                let title = self.input.trim().to_string();
                self.mode = Mode::Normal;
                if !title.is_empty() {
                    self.add_item(&title)?;
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => self.input.push(c),
            _ => {}
        }
        Ok(false)
    }

    fn last_item_row(&self) -> Option<usize> {
        self.rows
            .iter()
            .rposition(|r| matches!(r, Row::Item { .. }))
    }

    /// j/k — skip header rows, wrap nothing (pm stops at edges).
    fn move_sel(&mut self, delta: isize) {
        let mut i = self.sel as isize;
        loop {
            i += delta;
            if i < 0 || i as usize >= self.rows.len() {
                break;
            }
            if matches!(self.rows[i as usize], Row::Item { .. }) {
                self.sel = i as usize;
                break;
            }
        }
    }

    fn switch_board(&mut self, idx: usize) {
        self.board = Board::ALL[idx];
        self.rebuild_rows();
    }

    fn step_board(&mut self, delta: isize) {
        let idx = (Board::ALL
            .iter()
            .position(|b| *b == self.board)
            .unwrap_or(0) as isize
            + delta)
            .rem_euclid(Board::ALL.len() as isize) as usize;
        self.switch_board(idx);
    }

    /// `Space`/`dd` — the `todo done` verb: walk to done, remove the
    /// line (and its subtree), one commit.
    fn done_selected(&mut self) -> Result<(), String> {
        let id = match self.selected_id() {
            Some(id) => id,
            None => return Ok(()),
        };
        let loc = self
            .ledger
            .locate(id)
            .ok_or_else(|| format!("{id}: no such task"))?;
        if loc.backlog {
            self.status = format!("{id} is deferred — `p` promotes it first");
            return Ok(());
        }
        let mut item = self.ledger.take(&loc);
        items::walk_to(&mut item, Status::Done)?;
        self.ledger.save(&self.store)?;
        items::commit(
            &self.proj.worktree,
            &format!("chore: done {id}"),
            self.no_commit,
        )?;
        self.status = format!("done {id} — {}", item.title);
        self.rebuild_rows();
        self.reselect(None);
        Ok(())
    }

    /// `D` — `todo defer`: park the subtree into BACKLOG.md.
    fn defer_selected(&mut self) -> Result<(), String> {
        let id = match self.selected_id() {
            Some(id) => id,
            None => return Ok(()),
        };
        let loc = self
            .ledger
            .locate(id)
            .ok_or_else(|| format!("{id}: no such task"))?;
        if loc.backlog {
            self.status = format!("{id} is already deferred");
            return Ok(());
        }
        let mut item = self.ledger.take(&loc);
        items::walk_to(&mut item, Status::Deferred)?;
        items::walk_descendants_to(&mut item, Status::Deferred)?;
        self.ledger.put_backlog(item, None);
        self.ledger.save(&self.store)?;
        items::commit(
            &self.proj.worktree,
            &format!("chore: defer {id}"),
            self.no_commit,
        )?;
        self.status = format!("deferred {id} → BACKLOG.md");
        self.rebuild_rows();
        self.reselect(None);
        Ok(())
    }

    /// `p` — `backlog promote`: unpark the subtree into TODO.md.
    fn promote_selected(&mut self) -> Result<(), String> {
        let id = match self.selected_id() {
            Some(id) => id,
            None => return Ok(()),
        };
        let loc = self
            .ledger
            .locate(id)
            .ok_or_else(|| format!("{id}: no such task"))?;
        if !loc.backlog {
            self.status = format!("{id} isn't in BACKLOG.md");
            return Ok(());
        }
        let mut item = self.ledger.take(&loc);
        items::walk_to(&mut item, Status::Open)?;
        items::walk_descendants_to(&mut item, Status::Open)?;
        self.ledger.put_todo(item, None);
        self.ledger.save(&self.store)?;
        items::commit(
            &self.proj.worktree,
            &format!("chore: promote {id}"),
            self.no_commit,
        )?;
        self.status = format!("promoted {id} → TODO.md");
        self.rebuild_rows();
        self.reselect(Some(id));
        Ok(())
    }

    /// `Alt+↑/↓` — sibling reorder (pm's backlog priority move).
    fn reorder(&mut self, delta: isize) {
        let Some(id) = self.selected_id() else {
            return;
        };
        let Some(mut loc) = self.ledger.locate(id) else {
            return;
        };
        if self.ledger.move_within(&mut loc, delta) {
            if let Err(e) = self.ledger.save(&self.store).and_then(|_| {
                items::commit(
                    &self.proj.worktree,
                    &format!("chore: reorder {id}"),
                    self.no_commit,
                )
                .map(|_| ())
            }) {
                self.status = format!("reorder {id}: {e}");
                self.reload();
            } else {
                self.status = format!("reordered {id}");
                self.rebuild_rows();
                self.reselect(Some(id));
            }
        }
    }

    /// `a` Enter — `todo add`/`backlog add` for the active board.
    fn add_item(&mut self, title: &str) -> Result<(), String> {
        let id = self
            .store
            .alloc_id(ItemKind::Task)
            .map_err(|e| e.to_string())?;
        let item = match self.board {
            Board::Todo => Item::new(id, title).created(items::today()),
            Board::Backlog => Item::new(id, title)
                .status(Status::Deferred)
                .created(items::today()),
        };
        match self.board {
            Board::Todo => self.ledger.put_todo(item.clone(), None),
            Board::Backlog => self.ledger.put_backlog(item.clone(), None),
        }
        self.ledger.save(&self.store)?;
        items::commit(
            &self.proj.worktree,
            &format!("chore: add {id}"),
            self.no_commit,
        )?;
        self.status = format!("added {}", items::compact(&item));
        self.rebuild_rows();
        self.reselect(Some(id));
        Ok(())
    }
}

fn push_items(items: &[Item], depth: usize, rows: &mut Vec<Row>) {
    for item in items {
        rows.push(Row::Item {
            item: Box::new(item.clone()),
            depth,
        });
        push_items(&item.children, depth + 1, rows);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use myco_core::Section;

    fn t(n: u32) -> Item {
        Item::new(ItemId::new(ItemKind::Task, n), format!("t{n}"))
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    /// A model over a temp `project/` dir — `no_commit` keeps git
    /// out of the loop (autocommit short-circuits).
    fn model(populate: impl FnOnce(&mut Ledger)) -> Model {
        let dir = tempfile::tempdir().unwrap();
        let worktree = dir.path().join("project");
        std::fs::create_dir_all(&worktree).unwrap();
        let store = MdStore::new(Utf8PathBuf::from_path_buf(worktree.clone()).unwrap());
        let mut ledger = Ledger::load(&store).unwrap();
        populate(&mut ledger);
        let mut m = Model {
            proj: discover::Project {
                root: dir.path().to_path_buf(),
                worktree,
            },
            store,
            ledger,
            no_commit: true,
            board: Board::Todo,
            rows: Vec::new(),
            sel: 0,
            mode: Mode::Normal,
            input: String::new(),
            status: String::new(),
            pending_editor: None,
            pending: None,
        };
        m.rebuild_rows();
        // keep the tempdir alive for the test's duration
        std::mem::forget(dir);
        m
    }

    #[test]
    fn rows_flatten_sections_and_children() {
        let m = model(|l| {
            l.todo.sections.push(Section {
                name: Some("Now".into()),
                items: vec![t(1), t(2).child(t(4))],
            });
        });
        assert!(matches!(m.rows[0], Row::Header(_)));
        assert!(matches!(m.rows[1], Row::Item { ref item, depth: 0 } if item.id.number() == 1));
        assert!(matches!(m.rows[3], Row::Item { ref item, depth: 1 } if item.id.number() == 4));
        // cursor lands on the first item, not the header
        assert_eq!(m.sel, 1);
        assert_eq!(m.selected().unwrap().id.number(), 1);
    }

    #[test]
    fn j_k_skip_headers_and_stop_at_edges() {
        let mut m = model(|l| {
            l.todo.sections.push(Section::named("S"));
            l.todo.sections[0].items = vec![t(1)];
            l.todo.sections.push(Section::named("T"));
            l.todo.sections[1].items = vec![t(2)];
        });
        // rows: H S, t1, H T, t2 — j from t1 lands on t2
        m.key(key('j')).unwrap();
        assert_eq!(m.selected().unwrap().id.number(), 2);
        // edge: no wrap
        m.key(key('j')).unwrap();
        assert_eq!(m.selected().unwrap().id.number(), 2);
        m.key(key('k')).unwrap();
        assert_eq!(m.selected().unwrap().id.number(), 1);
    }

    #[test]
    fn gg_and_g_and_tab_switch() {
        let mut m = model(|l| {
            l.put_todo(t(1), None);
            l.put_todo(t(2), None);
            l.put_backlog(t(3).status(Status::Deferred), None);
        });
        m.key(key('G')).unwrap();
        assert_eq!(m.selected().unwrap().id.number(), 2);
        m.key(key('g')).unwrap();
        m.key(key('g')).unwrap();
        assert_eq!(m.selected().unwrap().id.number(), 1);
        m.key(key('2')).unwrap();
        assert_eq!(m.board, Board::Backlog);
        assert_eq!(m.selected().unwrap().id.number(), 3);
        m.key(key('1')).unwrap();
        assert_eq!(m.board, Board::Todo);
    }

    #[test]
    fn dd_remembers_the_prefix_and_dones() {
        let mut m = model(|l| l.put_todo(t(1), None));
        m.key(key('d')).unwrap();
        assert!(m.selected().is_some()); // still there — only armed
        m.key(key('d')).unwrap();
        assert!(m.selected().is_none());
        assert!(m.ledger.locate(ItemId::new(ItemKind::Task, 1)).is_none());
    }

    #[test]
    fn defer_and_promote_cross_boards() {
        let mut m = model(|l| l.put_todo(t(1), None));
        m.key(key('D')).unwrap();
        let loc = m.ledger.locate(ItemId::new(ItemKind::Task, 1)).unwrap();
        assert!(loc.backlog);
        m.key(key('2')).unwrap();
        m.key(key('p')).unwrap();
        let loc = m.ledger.locate(ItemId::new(ItemKind::Task, 1)).unwrap();
        assert!(!loc.backlog);
        assert_eq!(m.board, Board::Backlog); // board stays put
    }

    #[test]
    fn add_mode_commits_title() {
        let mut m = model(|_| {});
        m.key(key('a')).unwrap();
        for c in "ship it".chars() {
            m.key(key(c)).unwrap();
        }
        m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(m.mode, Mode::Normal);
        let item = m.selected().unwrap();
        assert_eq!(item.title, "ship it");
    }
}
