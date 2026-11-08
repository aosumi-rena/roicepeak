#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod app;
mod failure;
mod platform;

fn main() -> eframe::Result {
    let result = app::run();
    if let Err(error) = &result {
        failure::report(&error.to_string());
    }
    result
}
