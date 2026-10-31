+++
id = "I-013"
title = "TUI hardening: raw-mode leak, stale ITEMS, no force-quit"
status = "open"
created = "2026-10-31"
priority = "medium"
labels = ["audit"]
+++

enable_raw_mode before EnterAlternateScreen → raw-mode leak if latter fails (lib.rs:42-45); panic msg erased by alt-screen exit. files loaded once, never reload (app.rs:117,147). No Ctrl-C/quit outside Normal (app.rs:346-377). EDITOR 'code --wait' fails (lib.rs:80). Preview can't scroll (render.rs:250). Help omits board 3 (render.rs:346). Fix: guard ordering, panic hook, reload files, global Ctrl-C, split EDITOR, scroll, keymap doc.

Related: I-019 — the ITEMS→FILES rename + related-files view
lands on this board; P-005's mention-scan derives this link.
