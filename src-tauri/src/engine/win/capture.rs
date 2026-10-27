use super::Shared;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use velo_core::{config::even, library::write_atomic, thumb, CaptureMode, StartConfig};
use velo_director::{Point, Size};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITORINFO};
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::{
    AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder, VideoSettingsSubType,
};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, GraphicsCaptureItemType,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};
use windows_capture::window::Window;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// Something that can be captured, with where it sits on the desktop.
pub struct Item {
    kind: ItemKind,
    pub width: u32,
    pub height: u32,
    /// Desktop position of the top-left corner, to make mouse positions relative to the capture.
    pub origin: Point,
}

enum ItemKind {
    Monitor(Monitor),
    Window(Window),
}

impl Item {
    pub fn size(&self) -> Size {
        Size { w: self.width as f64, h: self.height as f64 }
    }
}

/// Finds the monitor or window the UI picked. Indexes match `sources::list_sources`.
pub fn resolve(mode: CaptureMode, index: usize) -> Result<Item, String> {
    match mode {
        CaptureMode::Display => {
            let monitors = Monitor::enumerate().map_err(|e| e.to_string())?;
            let monitor = *monitors.get(index).ok_or("That display is no longer available.")?;
            let (width, height) = (monitor.width().map_err(err)?, monitor.height().map_err(err)?);
            Ok(Item { origin: monitor_origin(&monitor), kind: ItemKind::Monitor(monitor), width, height })
        }
        CaptureMode::Window => {
            let windows = super::sources::capturable_windows();
            let window = *windows.get(index).ok_or("That window is no longer available.")?;
            let rect = window.rect().map_err(err)?;
            Ok(Item {
                width: (rect.right - rect.left).max(2) as u32,
                height: (rect.bottom - rect.top).max(2) as u32,
                origin: Point::new(rect.left as f64, rect.top as f64),
                kind: ItemKind::Window(window),
            })
        }
        CaptureMode::Region | CaptureMode::Camera => Err("This capture mode is coming soon.".into()),
    }
}

/// Starts capturing `item` with any handler.
pub fn start_item<H>(
    item: &Item,
    cursor: CursorCaptureSettings,
    min_interval: MinimumUpdateIntervalSettings,
    flags: H::Flags,
) -> Result<CaptureControl<H, Error>, String>
where
    H: GraphicsCaptureApiHandler<Error = Error> + Send + 'static,
    H::Flags: Send,
{
    match &item.kind {
        ItemKind::Monitor(m) => H::start_free_threaded(build_settings(*m, cursor, min_interval, flags)),
        ItemKind::Window(w) => H::start_free_threaded(build_settings(*w, cursor, min_interval, flags)),
    }
    .map_err(|e| format!("Could not start capture: {e}"))
}

fn build_settings<F, T>(item: T, cursor: CursorCaptureSettings, min_interval: MinimumUpdateIntervalSettings, flags: F) -> Settings<F, T>
where
    T: TryInto<GraphicsCaptureItemType>,
{
    Settings::new(
        item,
        cursor,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        min_interval,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        flags,
    )
}

// ---------- Recording ----------

pub struct Flags {
    shared: Arc<Shared>,
    path: PathBuf,
    thumbnail: PathBuf,
    width: u32,
    height: u32,
    fps: u32,
    bitrate: u32,
    audio: bool,
}

pub struct Handler {
    shared: Arc<Shared>,
    thumbnail: PathBuf,
    thumbs_taken: u8,
    first_ms: u64,
}

/// When the library picture is taken: at once (so even a very short clip has one), and again after
/// a moment, when the screen is more likely to show what the recording is about.
const SECOND_THUMBNAIL_AFTER_MS: u64 = 1_500;

impl GraphicsCaptureApiHandler for Handler {
    type Flags = Flags;
    type Error = Error;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let f = ctx.flags;
        let encoder = VideoEncoder::new(
            VideoSettingsBuilder::new(f.width, f.height)
                .sub_type(VideoSettingsSubType::H264)
                .bitrate(f.bitrate)
                .frame_rate(f.fps),
            AudioSettingsBuilder::default().disabled(!f.audio),
            ContainerSettingsBuilder::default(),
            &f.path,
        )?;
        *f.shared.encoder.lock() = Some(encoder);
        Ok(Self { shared: f.shared, thumbnail: f.thumbnail, thumbs_taken: 0, first_ms: 0 })
    }

    fn on_frame_arrived(&mut self, frame: &mut Frame, _control: InternalCaptureControl) -> Result<(), Self::Error> {
        if self.shared.stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        if let Some(encoder) = self.shared.encoder.lock().as_mut() {
            self.shared.mark_first_frame();
            encoder.send_frame(frame)?;
        }
        self.maybe_take_thumbnail(frame);
        Ok(())
    }
}

impl Handler {
    fn maybe_take_thumbnail(&mut self, frame: &mut Frame) {
        let now = self.shared.now_ms();
        if self.thumbs_taken == 0 {
            self.first_ms = now;
        }
        let due = match self.thumbs_taken {
            0 => true,
            1 => now.saturating_sub(self.first_ms) >= SECOND_THUMBNAIL_AFTER_MS,
            _ => false,
        };
        if !due {
            return;
        }
        self.thumbs_taken += 1;
        // A failed thumbnail must never affect the recording.
        let _ = save_thumbnail(frame, &self.thumbnail);
    }
}

fn save_thumbnail(frame: &mut Frame, path: &Path) -> Result<(), String> {
    let jpeg = frame_to_jpeg(frame, 640, 75)?;
    write_atomic(path, &jpeg).map_err(|e| e.to_string())
}

/// Shrinks a captured frame to a small JPEG.
pub fn frame_to_jpeg(frame: &mut Frame, max_width: u32, quality: u8) -> Result<Vec<u8>, String> {
    let buffer = frame.buffer().map_err(err)?;
    let (w, h) = (buffer.width(), buffer.height());
    let mut scratch = Vec::new();
    let pixels = buffer.as_nopadding_buffer(&mut scratch);
    thumb::jpeg_from_rgba(pixels, w, h, max_width, quality).map(|(bytes, ..)| bytes)
}

pub struct Started {
    pub control: CaptureControl<Handler, Error>,
    pub screen: Size,
    pub origin: Point,
}

pub fn start(cfg: &StartConfig, shared: &Arc<Shared>, path: &Path, thumbnail: &Path) -> Result<Started, String> {
    let item = resolve(cfg.mode, if cfg.mode == CaptureMode::Display { cfg.display } else { cfg.window })?;
    // Studio mode leaves the cursor out of the footage so it can be redrawn smoothly in the editor.
    let cursor = if !cfg.studio && cfg.cursor { CursorCaptureSettings::WithCursor } else { CursorCaptureSettings::WithoutCursor };
    let (width, height) = (even(item.width), even(item.height));
    let flags = Flags {
        shared: shared.clone(),
        path: path.to_path_buf(),
        thumbnail: thumbnail.to_path_buf(),
        width,
        height,
        fps: cfg.fps(),
        bitrate: cfg.bitrate_bps(width, height),
        audio: cfg.system || cfg.mic,
    };
    let control = start_item::<Handler>(&item, cursor, MinimumUpdateIntervalSettings::Default, flags)?;
    Ok(Started { control, screen: item.size(), origin: item.origin })
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn monitor_origin(monitor: &Monitor) -> Point {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    let ok = unsafe { GetMonitorInfoW(HMONITOR(monitor.as_raw_hmonitor()), &mut info) }.as_bool();
    if ok { Point::new(info.rcMonitor.left as f64, info.rcMonitor.top as f64) } else { Point::new(0.0, 0.0) }
}
