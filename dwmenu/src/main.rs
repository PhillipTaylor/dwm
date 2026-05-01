// dmenu - dynamic menu (Rust port of suckless dmenu 4.3.1, Wayland-only).

#![allow(non_upper_case_globals)]

mod input;
mod match_logic;
mod render;
mod state;

use std::process::ExitCode;

use wayland_client::{globals::registry_queue_init, Connection};

use crate::state::Menu;

const VERSION: &str = "4.3.1";

pub struct Args {
    pub fast: bool,
    pub case_insensitive: bool,
    pub topbar: bool,
    pub lines: i32,
    pub prompt: Option<String>,
    pub font: Option<String>,
    pub font_size: f32,
    pub normbg: u32,
    pub normfg: u32,
    pub selbg: u32,
    pub selfg: u32,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            fast: false,
            case_insensitive: false,
            topbar: true,
            lines: 0,
            prompt: None,
            font: None,
            font_size: 14.0,
            normbg: 0xffcccccc,
            normfg: 0xff000000,
            selbg: 0xff0066ff,
            selfg: 0xffffffff,
        }
    }
}

fn usage() -> ! {
    eprintln!("usage: dmenu [-b] [-f] [-i] [-l lines] [-p prompt] [-fn font.ttf]");
    eprintln!("             [-fs size] [-nb #rgb] [-nf #rgb] [-sb #rgb] [-sf #rgb] [-v]");
    std::process::exit(1);
}

fn parse_color(s: &str) -> u32 {
    let h = s.strip_prefix('#').unwrap_or(s);
    if h.len() != 6 {
        return 0xff000000;
    }
    let r = u32::from_str_radix(&h[0..2], 16).unwrap_or(0);
    let g = u32::from_str_radix(&h[2..4], 16).unwrap_or(0);
    let b = u32::from_str_radix(&h[4..6], 16).unwrap_or(0);
    0xff000000 | (r << 16) | (g << 8) | b
}

fn parse_args() -> Args {
    let mut a = Args::default();
    let argv: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < argv.len() {
        let s = &argv[i];
        match s.as_str() {
            "-v" => {
                println!("dmenu-{}, Rust port of suckless dmenu (Wayland)", VERSION);
                std::process::exit(0);
            }
            "-b" => a.topbar = false,
            "-f" => a.fast = true,
            "-i" => a.case_insensitive = true,
            _ if i + 1 == argv.len() => usage(),
            "-l" => { i += 1; a.lines = argv[i].parse().unwrap_or(0); }
            "-p" => { i += 1; a.prompt = Some(argv[i].clone()); }
            "-fn" => { i += 1; a.font = Some(argv[i].clone()); }
            "-fs" => { i += 1; a.font_size = argv[i].parse().unwrap_or(14.0); }
            "-nb" => { i += 1; a.normbg = parse_color(&argv[i]); }
            "-nf" => { i += 1; a.normfg = parse_color(&argv[i]); }
            "-sb" => { i += 1; a.selbg  = parse_color(&argv[i]); }
            "-sf" => { i += 1; a.selfg  = parse_color(&argv[i]); }
            _ => usage(),
        }
        i += 1;
    }
    a
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("dmenu: {}", e);
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let args = parse_args();

    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let mut menu = Menu::new(&conn, &globals, &qh, args)?;

    // dmenu reads stdin before grabbing the keyboard unless -f is set.
    if !menu.args.fast {
        menu.read_stdin()?;
    }

    // Wait until the layer surface is configured before processing keys.
    while !menu.configured {
        event_queue.blocking_dispatch(&mut menu)?;
        if menu.exit_code.is_some() {
            return Ok(ExitCode::from(menu.exit_code.unwrap()));
        }
    }

    if menu.args.fast {
        menu.read_stdin()?;
    }
    menu.recompute_geometry();
    menu.do_match(false);
    menu.draw(&qh)?;

    while menu.exit_code.is_none() {
        event_queue.blocking_dispatch(&mut menu)?;
    }
    Ok(ExitCode::from(menu.exit_code.unwrap()))
}
