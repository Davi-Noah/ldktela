// Sem console em release no Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(err) = ldktela_desktop_lib::run() {
        eprintln!("fatal: {err}");
        std::process::exit(1);
    }
}
