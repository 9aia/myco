//! The ledger — TODO.md + BACKLOG.md held together: task items
//! move between the two docs on status changes (`deferred` parks,
//! `done`/`cancelled` archive into DONE.md — see [`DoneDoc`]'s
//! locate/take for the way back). Pure doc manipulation —
//! loading/saving through a `Store` lives in `myco-ops`.

use crate::{BacklogDoc, DoneDoc, Item, ItemId, Section, TodoDoc};

/// TODO.md + BACKLOG.md as one in-memory ledger.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ledger {
    /// `TODO.md`.
    pub todo: TodoDoc,
    /// `BACKLOG.md`.
    pub backlog: BacklogDoc,
}

/// Which doc a task lives in — and its position inside. `path` is
/// the index trail into the section's item tree: `[2]` is the
/// third top-level item, `[2,0]` its first child.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TaskLoc {
    /// `true` = BACKLOG.md, `false` = TODO.md.
    pub backlog: bool,
    /// Section index.
    pub section: usize,
    /// Index path within the section — len ≥ 1; deeper than 1
    /// means a nested child.
    pub path: Vec<usize>,
}

impl TaskLoc {
    /// Is this a nested child (not a top-level line)?
    pub fn is_child(&self) -> bool {
        self.path.len() > 1
    }
}

impl Ledger {
    /// Both docs, in memory.
    pub fn new(todo: TodoDoc, backlog: BacklogDoc) -> Self {
        Ledger { todo, backlog }
    }

    /// Locate a task in either doc — top-level lines and nested
    /// children alike (`None` when it's not a task or doesn't
    /// exist).
    pub fn locate(&self, id: ItemId) -> Option<TaskLoc> {
        find_pos(&self.todo.sections, id)
            .map(|(section, path)| TaskLoc {
                backlog: false,
                section,
                path,
            })
            .or_else(|| {
                find_pos(&self.backlog.sections, id).map(|(section, path)| TaskLoc {
                    backlog: true,
                    section,
                    path,
                })
            })
    }

    /// The item at `loc` — `None` when `loc` is stale (the ledger
    /// changed since [`locate`][Ledger::locate] produced it) or
    /// hand-rolled out of bounds.
    pub fn item(&self, loc: &TaskLoc) -> Option<&Item> {
        let sections = if loc.backlog {
            &self.backlog.sections
        } else {
            &self.todo.sections
        };
        item_at(&sections.get(loc.section)?.items, &loc.path)
    }

    /// Mutable access to the item at `loc` — `None` on a stale loc.
    pub fn item_mut(&mut self, loc: &TaskLoc) -> Option<&mut Item> {
        let sections = if loc.backlog {
            &mut self.backlog.sections
        } else {
            &mut self.todo.sections
        };
        item_at_mut(&mut sections.get_mut(loc.section)?.items, &loc.path)
    }

    /// Remove the item at `loc` — the whole subtree goes with it
    /// (`done` on a parent removes its children too). An emptied
    /// *unsectioned* run is dropped (it can't even be emitted);
    /// an emptied named section stays — a bare `##` heading emits
    /// and parses fine, and the heading is user structure, not
    /// item data. `None` on a stale loc — nothing is removed.
    pub fn take(&mut self, loc: &TaskLoc) -> Option<Item> {
        let sections = if loc.backlog {
            &mut self.backlog.sections
        } else {
            &mut self.todo.sections
        };
        take_from(sections, loc.section, &loc.path)
    }

    /// Swap the item at `loc` with a sibling — `delta` -1 moves up,
    /// +1 down. Works on nested children (siblings share the
    /// parent). Returns false at the list edges and on a stale
    /// loc. `loc` follows the item to its new index.
    pub fn move_within(&mut self, loc: &mut TaskLoc, delta: isize) -> bool {
        let sections = if loc.backlog {
            &mut self.backlog.sections
        } else {
            &mut self.todo.sections
        };
        let Some((last_idx, parents)) = loc.path.split_last() else {
            return false;
        };
        let Some(section) = sections.get_mut(loc.section) else {
            return false;
        };
        let mut level = &mut section.items;
        for &idx in parents {
            match level.get_mut(idx) {
                Some(item) => level = &mut item.children,
                None => return false,
            }
        }
        let i = *last_idx as isize;
        let j = i + delta;
        if j < 0 || j as usize >= level.len() {
            return false;
        }
        level.swap(i as usize, j as usize);
        let last = loc.path.len() - 1;
        loc.path[last] = j as usize;
        true
    }

    /// Append to a TODO.md section: `section` names one (created
    /// when absent); `None` = the first section, else an
    /// unsectioned run.
    pub fn put_todo(&mut self, item: Item, section: Option<&str>) {
        push_item(&mut self.todo.sections, item, section);
    }

    /// Append to BACKLOG.md — same placement rule as [`put_todo`].
    pub fn put_backlog(&mut self, item: Item, section: Option<&str>) {
        push_item(&mut self.backlog.sections, item, section);
    }
}

impl DoneDoc {
    /// Locate an item anywhere in the archive — `(section, path)`
    /// indexes into [`DoneDoc::sections`] with the same trail
    /// [`Ledger::locate`] produces (`[2,0]` = first child of the
    /// third top-level item).
    pub fn locate(&self, id: ItemId) -> Option<(usize, Vec<usize>)> {
        find_pos(&self.sections, id)
    }

    /// Remove the node at `(section, path)` — the whole subtree
    /// goes with it (reopen restores children with their parent;
    /// `done remove` deletes them). Same emptied-section rule as
    /// [`Ledger::take`]: an unsectioned run can't emit empty, a
    /// named `##` heading is user structure and stays. `None` on
    /// a stale position — nothing is removed.
    pub fn take(&mut self, section: usize, path: &[usize]) -> Option<Item> {
        take_from(&mut self.sections, section, path)
    }
}

/// Shared removal behind [`Ledger::take`] and [`DoneDoc::take`] —
/// `path` is the index trail `find_pos` recorded.
fn take_from(sections: &mut Vec<Section>, section: usize, path: &[usize]) -> Option<Item> {
    let (last_idx, parents) = path.split_last()?;
    let section_ref = sections.get_mut(section)?;
    let mut level = &mut section_ref.items;
    for &idx in parents {
        level = &mut level.get_mut(idx)?.children;
    }
    if *last_idx >= level.len() {
        return None;
    }
    let item = level.remove(*last_idx);
    if path.len() == 1 && section_ref.name.is_none() && section_ref.items.is_empty() {
        sections.remove(section);
    }
    Some(item)
}

fn find_pos(sections: &[Section], id: ItemId) -> Option<(usize, Vec<usize>)> {
    sections.iter().enumerate().find_map(|(si, s)| {
        let mut path = Vec::new();
        find_path(&s.items, id, &mut path).then_some((si, path))
    })
}

/// Depth-first id search — pushes the index trail onto `path`.
/// Iterative: each explicit frame is (siblings, next index), so a
/// deep tree walks the heap, not the call stack.
fn find_path(items: &[Item], id: ItemId, path: &mut Vec<usize>) -> bool {
    path.clear();
    let mut frames: Vec<(&[Item], usize)> = vec![(items, 0)];
    while let Some((level, i)) = frames.last().copied() {
        if i >= level.len() {
            frames.pop();
            path.pop();
            continue;
        }
        if let Some(frame) = frames.last_mut() {
            frame.1 = i + 1;
        }
        let item = &level[i];
        path.push(i);
        if item.id == id {
            return true;
        }
        if item.children.is_empty() {
            path.pop();
        } else {
            frames.push((&item.children, 0));
        }
    }
    false
}

fn item_at<'a>(mut items: &'a [Item], path: &[usize]) -> Option<&'a Item> {
    let (last_idx, parents) = path.split_last()?;
    for &idx in parents {
        items = &items.get(idx)?.children;
    }
    items.get(*last_idx)
}

fn item_at_mut<'a>(mut items: &'a mut [Item], path: &[usize]) -> Option<&'a mut Item> {
    let (last_idx, parents) = path.split_last()?;
    for &idx in parents {
        items = &mut items.get_mut(idx)?.children;
    }
    items.get_mut(*last_idx)
}

fn push_item(sections: &mut Vec<Section>, item: Item, section: Option<&str>) {
    match section {
        Some(name) => {
            if let Some(s) = sections
                .iter_mut()
                .find(|s| s.name.as_deref() == Some(name))
            {
                s.items.push(item);
            } else {
                sections.push(Section {
                    name: Some(name.to_string()),
                    items: vec![item],
                });
            }
        }
        None => {
            if sections.is_empty() {
                sections.push(Section::unsectioned());
            }
            sections[0].items.push(item);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{ItemKind, Status};

    fn t(n: u32) -> Item {
        Item::new(ItemId::new(ItemKind::Task, n), format!("t{n}"))
    }

    fn tid(n: u32) -> ItemId {
        ItemId::new(ItemKind::Task, n)
    }

    #[test]
    fn ledger_moves_and_prunes() {
        let mut ledger = Ledger::default();
        ledger.put_todo(t(1), None);
        ledger.put_todo(t(2), Some("Now"));
        let loc = ledger.locate(tid(1)).unwrap();
        assert!(!loc.backlog);
        let item = ledger.take(&loc).unwrap();
        assert_eq!(item.id, tid(1));
        // emptied unsectioned run dropped
        assert!(ledger.todo.sections.iter().all(|s| !s.items.is_empty()));
        assert!(ledger.locate(tid(1)).is_none());
        ledger.put_backlog(item.status(Status::Deferred), None);
        let loc = ledger.locate(tid(1)).unwrap();
        assert!(loc.backlog);
    }

    #[test]
    fn take_keeps_an_emptied_named_section() {
        // a bare `##` heading emits and parses fine — removing its
        // last item must not delete user structure (I-021)
        let mut ledger = Ledger::default();
        ledger.put_todo(t(1), Some("Now"));
        let loc = ledger.locate(tid(1)).unwrap();
        ledger.take(&loc).unwrap();
        assert_eq!(ledger.todo.sections.len(), 1);
        assert_eq!(ledger.todo.sections[0].name.as_deref(), Some("Now"));
        assert!(ledger.todo.sections[0].items.is_empty());
        // …while an emptied unsectioned run is still dropped
        let mut ledger = Ledger::default();
        ledger.put_todo(t(1), None);
        let loc = ledger.locate(tid(1)).unwrap();
        ledger.take(&loc).unwrap();
        assert!(ledger.todo.sections.is_empty());
    }

    #[test]
    fn ledger_locates_and_takes_children() {
        let mut ledger = Ledger::default();
        ledger.put_todo(t(1).child(t(2).child(t(4))).child(t(3)), None);
        ledger.put_todo(t(5), None);
        // locate walks the whole tree — the path says where
        let loc = ledger.locate(tid(4)).unwrap();
        assert_eq!(loc.path, vec![0, 0, 0]);
        assert!(loc.is_child());
        assert_eq!(ledger.item(&loc).unwrap().id.number(), 4);
        // take on a child removes just that node
        ledger.take(&loc);
        let parent = &ledger.todo.sections[0].items[0];
        assert_eq!(parent.children[0].children.len(), 0);
        // take on a parent removes the whole subtree; sections
        // stay (a sibling remains)
        let loc = ledger.locate(tid(1)).unwrap();
        let gone = ledger.take(&loc).unwrap();
        assert_eq!(gone.children.len(), 2);
        assert!(ledger.locate(tid(2)).is_none());
        assert_eq!(ledger.todo.sections[0].items.len(), 1);
        // mutation in place reaches children too
        let loc = ledger.locate(tid(5)).unwrap();
        ledger.item_mut(&loc).unwrap().children.push(t(6));
        assert!(ledger.locate(tid(6)).is_some());
    }

    #[test]
    fn move_within_swaps_siblings() {
        let mut ledger = Ledger::default();
        ledger.put_todo(t(1), None);
        ledger.put_todo(t(2), None);
        ledger.put_todo(t(3).child(t(4)), None);
        // edges don't move
        let mut loc = ledger.locate(tid(1)).unwrap();
        assert!(!ledger.move_within(&mut loc, -1));
        // down swaps with the next sibling and updates loc
        assert!(ledger.move_within(&mut loc, 1));
        assert_eq!(loc.path, vec![1]);
        assert_eq!(ledger.todo.sections[0].items[1].id.number(), 1);
        // nested children reorder among themselves
        let mut loc = ledger.locate(tid(4)).unwrap();
        assert!(!ledger.move_within(&mut loc, -1));
    }

    #[test]
    fn done_doc_locate_and_take() {
        use crate::{DoneDoc, Status};
        let mut doc = DoneDoc::default();
        doc.sections.push(Section {
            name: None,
            items: vec![
                t(1).status(Status::Done).child(t(2).status(Status::Done)),
                t(3).status(Status::Cancelled),
            ],
        });
        doc.sections.push(Section::named("2026-11"));
        doc.sections[1].items.push(t(4).status(Status::Done));
        // locate finds nested children and later sections
        assert_eq!(doc.locate(tid(2)), Some((0, vec![0, 0])), "nested child");
        assert_eq!(doc.locate(tid(4)), Some((1, vec![0])));
        assert!(doc.locate(tid(9)).is_none());
        // take removes just the node — a nested child leaves its
        // parent archived
        let gone = doc.take(0, &[0, 0]).unwrap();
        assert_eq!(gone.id, tid(2));
        assert_eq!(doc.sections[0].items[0].children.len(), 0);
        // take on a top-level item drops an emptied unsectioned
        // section but keeps a named one
        let gone = doc.take(0, &[1]).unwrap();
        assert_eq!(gone.id, tid(3));
        assert_eq!(doc.sections.len(), 2); // t(1) still holds the run
        let gone = doc.take(0, &[0]).unwrap();
        assert_eq!(gone.id, tid(1));
        assert_eq!(doc.sections.len(), 1); // unsectioned dropped
        assert_eq!(doc.sections[0].name.as_deref(), Some("2026-11"));
        // stale positions are None, not panics
        assert!(doc.take(9, &[0]).is_none());
        assert!(doc.take(0, &[4]).is_none());
        assert!(doc.take(0, &[]).is_none());
    }

    #[test]
    fn stale_loc_is_none_not_a_panic() {
        let mut ledger = Ledger::default();
        ledger.put_todo(t(1), None);
        // a loc pointing past every bound — section, path
        // segment, and final index — must not panic
        for loc in [
            TaskLoc {
                backlog: false,
                section: 9,
                path: vec![0],
            },
            TaskLoc {
                backlog: false,
                section: 0,
                path: vec![9],
            },
            TaskLoc {
                backlog: false,
                section: 0,
                path: vec![0, 3],
            },
            // a loc can even carry an empty path — pub field
            TaskLoc {
                backlog: false,
                section: 0,
                path: vec![],
            },
            // wrong doc entirely
            TaskLoc {
                backlog: true,
                section: 0,
                path: vec![0],
            },
        ] {
            assert!(ledger.item(&loc).is_none(), "{loc:?}");
            assert!(ledger.item_mut(&loc).is_none(), "{loc:?}");
            assert!(ledger.take(&loc).is_none(), "{loc:?}");
            let mut owned = loc.clone();
            assert!(!ledger.move_within(&mut owned, 1), "{loc:?}");
        }
        // the ledger is untouched
        assert_eq!(ledger.todo.sections[0].items.len(), 1);
    }
}
