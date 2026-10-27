//! One-off snapshots and a low-rate live preview of a monitor or window.

use super::capture::{frame_to_jpeg, resolve, start_item, Error};
use crate::engine::FrameSink;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};
use velo_core::CaptureMode;
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::settings::{CursorCaptureSettings, MinimumUpdateIntervalSettings};

// ---------- One frame ----------

struct OneShot {
    tx: Sender<Result<Vec<u8>, String>>,
    max_width: u32,
}

impl GraphicsCaptureApiHandler for OneShot {
    type Flags = (Sender<Result<Vec<u8>, String>>, u32);
    type Error = Error;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { tx: ctx.flags.0, max_width: ctx.flags.1 })
    }

    fn on_frame_arrived(&mut self, frame: &mut Frame, control: InternalCaptureControl) -> Result<(), Self::Error> {
        let _ = self.tx.send(frame_to_jpeg(frame, self.max_width, 70));
        control.stop();
        Ok(())
    }
}

/// Grabs a single frame of a monitor or window as a small JPEG.
pub fn snapshot(mode: CaptureMode, index: usize, max_width: u32) -> Result<Vec<u8>, String> {
    let item = resolve(mode, index)?;
    let (tx, rx) = mpsc::channel();
    let control = start_item::<OneShot>(
        &item,
        CursorCaptureSettings::WithoutCursor,
        MinimumUpdateIntervalSettings::Default,
        (tx, max_width),
    )?;
    // A minimised window never produces a frame, so do not wait for ever.
    let result = rx.recv_timeout(Duration::from_millis(1_500)).map_err(|_| "No picture available.".to_string())?;
    let _ = control.stop();
    result
}

// ---------- Live preview ----------

struct Live {
    on_frame: FrameSink,
    last: Instant,
}

/// Frames per second shown in the preview. Low on purpose: it is a glance, not a recording.
const PREVIEW_INTERVAL: Duration = Duration::from_millis(200);

impl GraphicsCaptureApiHandler for Live {
    type Flags = FrameSink;
    type Error = Error;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { on_frame: ctx.flags, last: Instant::now() - PREVIEW_INTERVAL })
    }

    fn on_frame_arrived(&mut self, frame: &mut Frame, _control: InternalCaptureControl) -> Result<(), Self::Error> {
        if self.last.elapsed() < PREVIEW_INTERVAL {
            return Ok(());
        }
        self.last = Instant::now();
        if let Ok(jpeg) = frame_to_jpeg(frame, 960, 65) {
            (self.on_frame)(jpeg);
        }
        Ok(())
    }
}

/// A running live preview. Dropping it stops the capture.
pub struct Preview {
    control: Option<CaptureControl<Live, Error>>,
}

impl Preview {
    pub fn start(mode: CaptureMode, index: usize, on_frame: FrameSink) -> Result<Self, String> {
        let item = resolve(mode, index)?;
        let control = start_item::<Live>(
            &item,
            CursorCaptureSettings::WithCursor,
            MinimumUpdateIntervalSettings::Custom(PREVIEW_INTERVAL),
            on_frame,
        )?;
        Ok(Self { control: Some(control) })
    }

    fn halt(&mut self) {
        if let Some(control) = self.control.take() {
            let _ = control.stop();
        }
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        self.halt();
    }
}
