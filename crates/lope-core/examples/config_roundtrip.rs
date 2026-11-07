//! A3 helper: `config_roundtrip IN OUT` = Rust load(IN) then save(OUT).
//! Prints NONE if load rejects the file (Python returns None).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    match lope_core::config::load(std::path::Path::new(&a[1])) {
        Some(cfg) => {
            lope_core::config::save(&cfg, std::path::Path::new(&a[2])).expect("save");
            println!("SAVED");
        }
        None => println!("NONE"),
    }
}
