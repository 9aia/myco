//! myco-tui — interactive board TUI over `myco-ops`. Standalone
//! entry; `myco ui` embeds the same `myco_tui::run`.

fn main() {
    if let Err(e) = myco_tui::run(None, None, false) {
        eprintln!("myco-tui: error: {e}");
        std::process::exit(1);
    }
}
