//! Interactive confirmation for destructive verbs — a tty prompt
//! gated by `--yes`/`$MYCO_YES`. Scripts, agents, `--json` pipes,
//! and the wire surface (`serve`/`batch`) never block: they either
//! pass `--yes` or simply aren't a terminal.

use std::io::{IsTerminal, Write};

/// Should this invocation prompt before a destructive action?
/// Only on an interactive terminal without `--yes` (`a.flag`) or
/// `$MYCO_YES` — everything else proceeds silently.
pub fn needed(yes: bool) -> bool {
    !yes && std::env::var_os("MYCO_YES").is_none()
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
}

/// Ask `prompt` on stderr (stdout stays clean for `--json`), read
/// one line on stdin. `y`/`yes` confirms; anything else —
/// including EOF — cancels.
pub fn ask(prompt: &str) -> bool {
    eprint!("{prompt} [y/N] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(_) => matches!(line.trim().to_lowercase().as_str(), "y" | "yes"),
        Err(_) => false,
    }
}
