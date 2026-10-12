//! Prints the Warp tab config Peekaboo would write for a session (manual check).
fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("dir"));
    let stem = peekaboo_core::terminal::write_warp_tab_config(&dir, "/Users/me/my project", "24bd582e-3f62-428d-8eeb-7aae27829afb").unwrap();
    print!("{}", std::fs::read_to_string(dir.join(format!("{stem}.toml"))).unwrap());
}
