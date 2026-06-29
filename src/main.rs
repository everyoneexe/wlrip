use std::sync::Arc;

use utils::{Channel, Mutex, log::LoggerOptions};

use crate::{
    config::{BASE_PATH, CONFIG_PATH, DEFAULT_CONFIG_NAME, parse_config},
    data::Data,
    os::mouse::check_uinput,
    ui::{app::App, grenades::read_grenades, run_overlay},
};

mod config;
mod constants;
mod cs2;
mod data;
mod game;
mod math;
mod message;
mod os;
mod parser;
mod ui;

#[cfg(not(target_os = "linux"))]
compile_error!("only linux is supported.");

fn main() {
    utils::log::init(
        LoggerOptions::default()
            .file(BASE_PATH.join("wlrip.log"))
            .truncate(true),
        |w, rec| {
            writeln!(
                w,
                "[{}] [{}:{}] {}",
                rec.level, rec.location.file, rec.location.line, rec.args
            )
        },
    )
    .expect("failed to initialize logger");

    // uinput is only needed for mouse-driven features (aimbot, triggerbot,
    // rcs). If it's unavailable we still run in ESP-only mode, so just warn
    // instead of bailing out.
    if !check_uinput() {
        utils::error!("uinput unavailable: running in ESP-only mode (no aimbot/triggerbot/rcs).");
    }

    // the layer-shell overlay needs the wayland display to connect on its own
    // thread; the gui (winit) reads it from the environment directly.
    let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();

    let (channel_gui, channel_game) = Channel::new();
    let data = Arc::new(Mutex::new(Data::default()));
    let data_game = data.clone();

    std::thread::spawn(move || {
        game::GameManager::new(channel_game, data_game).run();
    });

    // overlay render thread (wlr-layer-shell). config + grenade updates are
    // pushed from the gui thread over this channel.
    let (overlay_tx, overlay_rx) = std::sync::mpsc::channel();
    if let Some(wayland_display) = wayland_display {
        let data_overlay = data.clone();
        let config = parse_config(&CONFIG_PATH.join(DEFAULT_CONFIG_NAME));
        let grenades = read_grenades();
        std::thread::spawn(move || {
            run_overlay(wayland_display, data_overlay, config, grenades, overlay_rx);
        });
    } else {
        utils::warn!("WAYLAND_DISPLAY not set, overlay disabled");
    }

    let event_loop = match winit::event_loop::EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(err) => {
            utils::error!("failed to create event loop: {err}");
            return;
        }
    };
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
    let mut app = App::new(channel_gui, overlay_tx, data);
    event_loop.run_app(&mut app).unwrap();
}
