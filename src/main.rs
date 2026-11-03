#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod app;
mod platform;

fn main() -> eframe::Result {
    app::run()
}
