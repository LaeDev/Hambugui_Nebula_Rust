//! The GUI without a console window: what a double-click on the packaged
//! `nebula-gui.exe` runs.
//!
//! `nebula.exe gui` keeps its terminal and its stderr mirror; this binary has
//! neither, so the same log text goes to `<config dir>/nebula-gui.log` and a
//! startup failure is reported there rather than to a console nobody has.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    if let Err(err) = nebula::run_gui_windowless() {
        log::error!("{err}");
        std::process::exit(1);
    }
}
