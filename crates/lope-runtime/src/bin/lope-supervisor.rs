//! Stand-alone supervisor binary (tests and development). In production the
//! `lope` binary re-executes itself with argv0 `lope-supervisor`.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(lope_runtime::supervisor::main(&args));
}
