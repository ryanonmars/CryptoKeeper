use std::io::{self, IsTerminal, Write};

const CLEAR: &str = "\x1b[2J\x1b[H";

pub fn setup_app_theme(clear_screen: bool) -> Option<()> {
    if !io::stdout().is_terminal() {
        return None;
    }
    let mut out = io::stdout();
    set_title("CryptoKeeper");
    if clear_screen {
        let _ = out.write_all(CLEAR.as_bytes());
    }
    let _ = out.flush();
    Some(())
}

fn set_title(title: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(b"\x1b]0;");
    let _ = out.write_all(title.as_bytes());
    let _ = out.write_all(b"\x07");
}
