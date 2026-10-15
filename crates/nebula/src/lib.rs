//! Nebula's two front ends, behind three entry points.
//!
//! The package ships two binaries — `nebula` (console) and `nebula-gui`
//! (windowless) — and both are three lines over this library. The modules stay
//! private so the binaries reach exactly what the old single-binary crate
//! reached, and so the Slint types `slint::include_modules!` generates in
//! [`gui`] are not re-exported as this crate's public API: the royalty-free
//! licence Nebula ships Slint under does not permit an application to expose
//! them (§3).

mod cli;
mod gui;

pub use cli::run as run_cli;
pub use gui::{run as run_gui, run_windowless as run_gui_windowless};
