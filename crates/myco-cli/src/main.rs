//! myco — compat CLI driver. The engine is the `myco` lib
//! (src/lib.rs); this binary is just `myco::run()`.

fn main() {
    std::process::exit(myco::run());
}
