use std::{
    sync::{Arc, mpsc::Receiver},
    time::{Duration, Instant},
};

use utils::Mutex;

use crate::{
    config::Config,
    data::Data,
    message::OverlayMessage,
    ui::{
        grenades::GrenadeList, overlay::OverlayRenderer, wayland_overlay::WaylandOverlay,
    },
};

pub mod app;
pub mod color;
mod drag_range;
pub mod grenades;
mod gui;
mod overlay;
mod trail;
mod wayland_overlay;
mod window_context;

/// Entry point for the overlay render thread. Owns the wlr-layer-shell surface
/// and an [`OverlayRenderer`], draws the ESP every frame, and applies config /
/// grenade updates pushed from the gui thread.
pub fn run_overlay(
    wayland_display: String,
    data: Arc<Mutex<Data>>,
    config: Config,
    grenades: GrenadeList,
    rx: Receiver<OverlayMessage>,
) {
    let Some(mut overlay) = WaylandOverlay::new(&wayland_display) else {
        utils::error!("failed to create wayland overlay");
        return;
    };

    let mut renderer = OverlayRenderer::new(data, config, grenades);

    loop {
        let frame_start = Instant::now();

        // drain pending config / grenade updates
        while let Ok(message) = rx.try_recv() {
            renderer.apply(message);
        }

        overlay.begin_frame();
        overlay.render(|ui| renderer.overlay(ui));
        overlay.end_frame();

        let fps = renderer.config.fps.max(1) as f32;
        let frame_time = Duration::from_secs_f32(1.0 / fps);
        let elapsed = frame_start.elapsed();
        if let Some(remaining) = frame_time.checked_sub(elapsed) {
            std::thread::sleep(remaining);
        }
    }
}
