//! Types a test string into the Terminal.app tab on the given tty (manual check of the typing path).
fn main() {
    let tty = std::env::args().nth(1).expect("tty");
    println!("{:?}", peekaboo_core::terminal::type_into_session(&tty, "say \"hi\" \\ ok\nnext"));
}
