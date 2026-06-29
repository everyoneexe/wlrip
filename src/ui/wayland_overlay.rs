use std::sync::Arc;

use egui::{FontData, FontDefinitions};
use egui_glow::glow::{self, HasContext as _};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_region, wl_surface},
};

struct WaylandState {
    registry_state: RegistryState,
    _compositor_state: CompositorState,
    output_state: OutputState,
    _layer_shell: LayerShell,
    layer_surface: Option<LayerSurface>,
    /// logical surface size reported by the compositor's configure
    width: u32,
    height: u32,
    /// integer output scale (1 = unscaled, 2 = hidpi, ...)
    scale: i32,
    configured: bool,
}

pub struct WaylandOverlay {
    egl_display: khronos_egl::Display,
    egl_context: khronos_egl::Context,
    egl_surface: khronos_egl::Surface,
    egl: Arc<khronos_egl::DynamicInstance<khronos_egl::EGL1_4>>,
    _wl_egl_window: wayland_egl::WlEglSurface,
    _connection: Connection,
    event_queue: wayland_client::EventQueue<WaylandState>,
    state: WaylandState,
    glow: Arc<glow::Context>,
    egui_ctx: egui::Context,
    painter: egui_glow::Painter,
    width: u32,
    height: u32,
}

impl WaylandOverlay {
    pub fn new(wayland_display: &str) -> Option<Self> {
        let _ = wayland_display;
        let connection = Connection::connect_to_env().ok()?;

        let (globals, mut event_queue) = registry_queue_init(&connection).ok()?;
        let qh = event_queue.handle();

        let compositor_state = CompositorState::bind(&globals, &qh).ok()?;
        let layer_shell = LayerShell::bind(&globals, &qh).ok()?;
        let output_state = OutputState::new(&globals, &qh);
        let registry_state = RegistryState::new(&globals);

        let surface = compositor_state.create_surface(&qh);

        let layer_surface = layer_shell.create_layer_surface(
            &qh,
            surface,
            Layer::Overlay,
            Some("wlrip-esp"),
            None,
        );

        layer_surface.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer_surface.set_exclusive_zone(-1);
        layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);

        // Set empty input region so all clicks pass through
        let wl_surface = layer_surface.wl_surface();
        let region = compositor_state.wl_compositor().create_region(&qh, ());
        wl_surface.set_input_region(Some(&region));
        wl_surface.commit();

        let mut state = WaylandState {
            registry_state,
            _compositor_state: compositor_state,
            output_state,
            _layer_shell: layer_shell,
            layer_surface: Some(layer_surface),
            width: 1920,
            height: 1080,
            scale: 1,
            configured: false,
        };

        // Wait for configure event
        for _ in 0..20 {
            event_queue.blocking_dispatch(&mut state).ok()?;
            if state.configured {
                break;
            }
        }
        if !state.configured {
            utils::warn!("wayland layer surface not configured");
            return None;
        }

        // The compositor reports a logical surface size; on fractional/hidpi
        // outputs the actual pixel buffer must be scaled up. ESP coordinates
        // come from the game in physical pixels, so we render the overlay at
        // physical resolution and tell the compositor the buffer scale.
        let scale = state.scale.max(1);
        let width = state.width * scale as u32;
        let height = state.height * scale as u32;
        {
            let layer_ref = state.layer_surface.as_ref().unwrap();
            let wl_surface = layer_ref.wl_surface();
            wl_surface.set_buffer_scale(scale);
            wl_surface.commit();
        }
        utils::info!(
            "wayland overlay: {}x{} (logical {}x{} scale {})",
            width, height, state.width, state.height, scale
        );

        // EGL setup
        let egl = unsafe {
            khronos_egl::DynamicInstance::<khronos_egl::EGL1_4>::load_required_from_filename("libEGL.so.1").ok()?
        };
        let egl = Arc::new(egl);

        let wl_display_ptr = connection.backend().display_ptr() as khronos_egl::NativeDisplayType;
        let egl_display = unsafe { egl.get_display(wl_display_ptr) }.unwrap();
        egl.initialize(egl_display).ok()?;

        let config_attribs = [
            khronos_egl::RED_SIZE, 8,
            khronos_egl::GREEN_SIZE, 8,
            khronos_egl::BLUE_SIZE, 8,
            khronos_egl::ALPHA_SIZE, 8,
            khronos_egl::SURFACE_TYPE, khronos_egl::WINDOW_BIT,
            khronos_egl::RENDERABLE_TYPE, khronos_egl::OPENGL_ES2_BIT,
            khronos_egl::NONE,
        ];

        let egl_config = egl.choose_first_config(egl_display, &config_attribs).ok()?.unwrap();

        let context_attribs = [
            khronos_egl::CONTEXT_CLIENT_VERSION, 2,
            khronos_egl::NONE,
        ];

        let egl_context = egl.create_context(egl_display, egl_config, None, &context_attribs).ok()?;

        // Create wl_egl_window from the layer surface
        let layer_ref = state.layer_surface.as_ref().unwrap();
        let wl_surface = layer_ref.wl_surface();
        let wl_egl_window = wayland_egl::WlEglSurface::new(
            wl_surface.id(),
            width as i32,
            height as i32,
        ).ok()?;

        let egl_surface = unsafe {
            egl.create_window_surface(
                egl_display,
                egl_config,
                wl_egl_window.ptr() as khronos_egl::NativeWindowType,
                None,
            ).ok()?
        };

        egl.make_current(egl_display, Some(egl_surface), Some(egl_surface), Some(egl_context)).ok()?;
        let _ = egl.swap_interval(egl_display, 0);

        // Create glow context
        let egl_ref = egl.clone();
        let glow = unsafe {
            glow::Context::from_loader_function(|s| {
                let c_str = std::ffi::CString::new(s).unwrap();
                egl_ref.get_proc_address(c_str.as_c_str().to_str().unwrap())
                    .map(|f| f as *const std::ffi::c_void)
                    .unwrap_or(std::ptr::null())
            })
        };
        let glow = Arc::new(glow);

        // Create egui context and painter
        let egui_ctx = egui::Context::default();
        prep_ctx(&egui_ctx);

        let painter = egui_glow::Painter::new(glow.clone(), "", None, true)
            .expect("failed to create egui painter for wayland overlay");

        utils::info!("wayland layer shell overlay created");

        Some(Self {
            _connection: connection,
            event_queue,
            state,
            egl_display,
            egl_context,
            egl_surface,
            egl,
            _wl_egl_window: wl_egl_window,
            glow,
            egui_ctx,
            painter,
            width,
            height,
        })
    }

    pub fn begin_frame(&mut self) -> bool {
        // If the compositor disconnected (e.g. on shutdown), dispatching the
        // queue errors out. Bail so the loop can stop instead of spinning on a
        // dead connection.
        if self.event_queue.dispatch_pending(&mut self.state).is_err() {
            utils::error!("wayland dispatch failed, stopping overlay");
            return false;
        }

        let scale = self.state.scale.max(1);
        let phys_width = self.state.width * scale as u32;
        let phys_height = self.state.height * scale as u32;
        if phys_width != self.width || phys_height != self.height {
            self.width = phys_width;
            self.height = phys_height;
            if let Some(layer) = self.state.layer_surface.as_ref() {
                let wl_surface = layer.wl_surface();
                wl_surface.set_buffer_scale(scale);
                wl_surface.commit();
            }
            self._wl_egl_window.resize(self.width as i32, self.height as i32, 0, 0);
        }

        if self
            .egl
            .make_current(
                self.egl_display,
                Some(self.egl_surface),
                Some(self.egl_surface),
                Some(self.egl_context),
            )
            .is_err()
        {
            utils::error!("egl make_current failed, stopping overlay");
            return false;
        }

        unsafe {
            self.glow.viewport(0, 0, self.width as i32, self.height as i32);
            self.glow.clear_color(0.0, 0.0, 0.0, 0.0);
            self.glow.clear(glow::COLOR_BUFFER_BIT);
        }

        true
    }

    pub fn render(&mut self, run_ui: impl FnMut(&mut egui::Ui)) {
        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(self.width as f32, self.height as f32),
            )),
            ..Default::default()
        };

        let full_output = self.egui_ctx.run_ui(raw_input, run_ui);
        let clipped_primitives = self.egui_ctx.tessellate(
            full_output.shapes,
            full_output.pixels_per_point,
        );

        for (id, delta) in &full_output.textures_delta.set {
            self.painter.set_texture(*id, delta);
        }

        self.painter.paint_primitives(
            [self.width, self.height],
            full_output.pixels_per_point,
            &clipped_primitives,
        );

        for id in &full_output.textures_delta.free {
            self.painter.free_texture(*id);
        }
    }

    pub fn end_frame(&self) -> bool {
        // A failed swap usually means the wayland display / EGL surface went
        // away (e.g. the compositor tore down our layer surface). Report it so
        // the render loop can exit cleanly instead of panicking the thread.
        match self.egl.swap_buffers(self.egl_display, self.egl_surface) {
            Ok(()) => true,
            Err(err) => {
                utils::error!("egl swap_buffers failed, stopping overlay: {err:?}");
                false
            }
        }
    }
}

impl Drop for WaylandOverlay {
    fn drop(&mut self) {
        self.painter.destroy();
        let _ = self.egl.destroy_surface(self.egl_display, self.egl_surface);
        let _ = self.egl.destroy_context(self.egl_display, self.egl_context);
    }
}

// --- SCTK delegate implementations ---

impl CompositorHandler for WaylandState {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, new_factor: i32) {
        self.scale = new_factor.max(1);
    }
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for WaylandState {
    fn output_state(&mut self) -> &mut OutputState { &mut self.output_state }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for WaylandState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {}

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        if configure.new_size.0 > 0 {
            self.width = configure.new_size.0;
        }
        if configure.new_size.1 > 0 {
            self.height = configure.new_size.1;
        }
        self.configured = true;
    }
}

impl ProvidesRegistryState for WaylandState {
    fn registry(&mut self) -> &mut RegistryState { &mut self.registry_state }
    registry_handlers![OutputState];
}

delegate_compositor!(WaylandState);
delegate_output!(WaylandState);
delegate_layer!(WaylandState);
delegate_registry!(WaylandState);

impl Dispatch<wl_region::WlRegion, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &wl_region::WlRegion,
        _: <wl_region::WlRegion as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {}
}

fn prep_ctx(ctx: &egui::Context) {
    let fira_sans = include_bytes!("../../resources/FiraSansIcons.ttf");
    let cs2_icons = include_bytes!("../../resources/CS2EquipmentIcons.ttf");
    let mut font_definitions = FontDefinitions::default();
    font_definitions.font_data.insert(
        String::from("fira_sans"),
        Arc::new(FontData::from_static(fira_sans)),
    );
    font_definitions.font_data.insert(
        String::from("cs2_icons"),
        Arc::new(FontData::from_static(cs2_icons)),
    );

    font_definitions
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, String::from("fira_sans"));
    font_definitions
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .insert(0, String::from("cs2_icons"));

    ctx.set_fonts(font_definitions);
}
