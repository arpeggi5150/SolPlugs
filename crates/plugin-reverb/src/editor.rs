//! The plugin's editor window: an egui-baseview child window showing the shared UI.

use std::sync::Arc;
use std::time::Duration;

use egui_baseview::baseview::dpi::LogicalSize;
use egui_baseview::{EguiWindow, EguiWindowSettings, Frame, RepaintNotifier};
use raw_window_handle::HasWindowHandle;
use solplugs_reverb_app::engine::Engine;
use solplugs_reverb_app::ui::{UiLocal, show};

/// Default editor size, logical pixels.
pub const DEFAULT_SIZE: (f64, f64) = (
    solplugs_reverb_app::ui::DEFAULT_SIZE.0 as f64,
    solplugs_reverb_app::ui::DEFAULT_SIZE.1 as f64,
);
/// Smallest usable size.
pub const MIN_SIZE: (f64, f64) = (
    solplugs_reverb_app::ui::MIN_SIZE.0 as f64,
    solplugs_reverb_app::ui::MIN_SIZE.1 as f64,
);

/// Called once per editor frame on the UI thread (the VST3 glue uses it to tell the host
/// about latency changes).
pub type FrameHook = Box<dyn FnMut(&Engine) + Send>;

struct EditorApp {
    engine: Arc<Engine>,
    local: UiLocal,
    repaint: RepaintNotifier,
    hook: Option<FrameHook>,
}

impl egui_baseview::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        show(ui, &self.engine, &mut self.local);
        if let Some(h) = self.hook.as_mut() {
            h(&self.engine);
        }
        // Keep the meters moving: ask for the next frame (polled at ~30 fps).
        self.repaint.request_repaint();
    }
}

fn settings(size: (f64, f64), repaint: &RepaintNotifier) -> EguiWindowSettings {
    EguiWindowSettings::new()
        .with_title(crate::NAME)
        .with_size(LogicalSize {
            width: size.0.max(MIN_SIZE.0),
            height: size.1.max(MIN_SIZE.1),
        })
        .with_min_size(Some(LogicalSize {
            width: MIN_SIZE.0,
            height: MIN_SIZE.1,
        }))
        .with_resizable(true)
        .with_repaint_notifier(repaint.clone())
        .with_repaint_poll_interval(Duration::from_millis(33))
}

/// Opens the editor as a child of `parent`, at `size` logical pixels.
pub fn open(
    parent: &impl HasWindowHandle,
    engine: Arc<Engine>,
    size: (f64, f64),
    hook: Option<FrameHook>,
) -> Result<egui_baseview::baseview::Window, String> {
    let repaint = RepaintNotifier::new();
    let settings = settings(size, &repaint).with_parent(parent);
    let window = EguiWindow::create(
        settings,
        EditorApp {
            engine,
            local: UiLocal::default(),
            repaint,
            hook,
        },
    )
    .map_err(|e| e.to_string())?;
    // baseview creates child windows hidden and only draws on WM_PAINT, so a window that is
    // never shown never runs a frame. CLAP hosts call `gui.show`, but VST3 has no such call:
    // without this the VST3 editor stays blank.
    let _ = window.show();
    Ok(window)
}

/// Opens the editor as its own top-level window (development and screenshots). Asks for
/// a plain framebuffer (no alpha, no sRGB) so it also opens on virtual displays.
pub fn open_standalone(engine: Arc<Engine>) -> Result<egui_baseview::baseview::Window, String> {
    let repaint = RepaintNotifier::new();
    let mut graphics = egui_baseview::GraphicsConfig::default();
    graphics.gl_config.alpha_bits = 0;
    graphics.gl_config.srgb = false;
    graphics.gl_config.stencil_bits = 0;
    EguiWindow::create(
        settings(DEFAULT_SIZE, &repaint).with_graphics_config(graphics),
        EditorApp {
            engine,
            local: UiLocal::default(),
            repaint,
            hook: None,
        },
    )
    .map_err(|e| e.to_string())
}

/// A parent window known only by its raw handle (what VST3 hands us).
#[cfg(windows)]
pub struct RawParent(pub std::num::NonZeroIsize);

#[cfg(windows)]
impl HasWindowHandle for RawParent {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let raw = raw_window_handle::RawWindowHandle::Win32(
            raw_window_handle::Win32WindowHandle::new(self.0),
        );
        // SAFETY: the host guarantees the HWND it passed to `IPlugView::attached` stays valid
        // until it calls `removed`, and the editor window is closed in `removed`.
        #[allow(unsafe_code)]
        unsafe {
            Ok(raw_window_handle::WindowHandle::borrow_raw(raw))
        }
    }
}
