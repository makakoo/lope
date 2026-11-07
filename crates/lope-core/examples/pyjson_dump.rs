//! Differential-test helper: reads one JSON document per stdin line and
//! prints it in the four json.dumps flavours Lope uses, tab-separated.
use lope_core::pyjson::{dumps, loads, Opts};
use std::io::BufRead;

fn main() {
    let flavours = [Opts::default_dumps(), Opts::compact_sorted(), Opts::pretty_sorted(), Opts::pretty()];
    for line in std::io::stdin().lock().lines() {
        let v = loads(&line.unwrap()).expect("valid json");
        let outs: Vec<String> = flavours.iter().map(|o| dumps(&v, o).replace('\n', "\\n")).collect();
        println!("{}", outs.join("\t"));
    }
}
