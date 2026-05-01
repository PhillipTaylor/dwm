// dwm - dynamic window manager (Rust port of suckless dwm 5.8.2 + pertag patch).
// Wayland compositor based on the smithay framework.
//
// LINUX-ONLY: depends on libudev/libdrm/libgbm/libinput/libseat via smithay
// backends. Will not compile on macOS or Windows.

#![allow(non_upper_case_globals, non_snake_case, dead_code, clippy::too_many_arguments)]

mod bar;
mod client;
mod config;
mod dwm_logic;
mod grabs;
mod handlers;
mod input;
mod monitor;
mod state;
mod udev;

use std::process::ExitCode;

const VERSION: &str = "5.8.2";

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    if argv.len() == 2 && argv[1] == "-v" {
        eprintln!("dwm-{}, Rust port of suckless dwm (Wayland compositor)", VERSION);
        return ExitCode::from(0);
    }
    if argv.len() != 1 {
        eprintln!("usage: dwm [-v]");
        return ExitCode::from(1);
    }

    init_logging();

    match crate::udev::run_udev() {
        Ok(()) => ExitCode::from(0),
        Err(e) => {
            tracing::error!("dwm: fatal: {}", e);
            ExitCode::from(1)
        }
    }
}

fn init_logging() {
    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new("info"))
            .init();
    }
}
