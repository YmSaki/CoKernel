#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod app;

fn main() {
    #[cfg(windows)]
    if let Err(error) = app::run() {
        eprintln!("CoKernel Desktop could not start: {error}");
        std::process::exit(1);
    }
    #[cfg(not(windows))]
    eprintln!("CoKernel Desktop requires Windows; Runtime runs in Linux.");
}
