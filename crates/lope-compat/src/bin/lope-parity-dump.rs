//! `lope-parity-dump [--coverage]`: print the clap-side projection used by the
//! A1 parity diff, or the native coverage table and its fingerprint.

fn main() {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        None => {
            let v = lope_compat::dump::projection();
            println!("{}", serde_json::to_string_pretty(&v).expect("serialise"));
        }
        Some("--coverage") => {
            println!("fingerprint {}", lope_compat::coverage::fingerprint());
            for c in lope_compat::coverage::COVERAGE {
                println!("{:<8} lope {:<24} args: [{}]", c.since, c.path.join(" "), c.args.join(", "));
            }
        }
        Some(other) => {
            eprintln!("usage: lope-parity-dump [--coverage] (got {other:?})");
            std::process::exit(2);
        }
    }
}
