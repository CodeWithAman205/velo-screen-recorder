//! Windows recording engine: Graphics Capture for video, WASAPI for sound, low-level hooks for
//! mouse and keyboard activity. One `Recording` owns all of it for the length of a take.

mod audio;
mod capture;
mod grab;
mod input;
mod sources;

use super::{now_ms, output_folder, LevelsSink};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use velo_core::library::DataDirs;
use velo_core::{CaptureMode, Clock, Entry, Project, StartConfig};
use velo_director::{InputEvent, Size};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows_capture::capture::CaptureControl;
use windows_capture::encoder::VideoEncoder;

pub use grab::{snapshot, Preview};
pub use sources::list_sources;

const NO_FRAME: u64 = u64::MAX;

/// State shared by the capture, audio and input threads of one recording.
pub(crate) struct Shared {
    pub clock: Mutex<Clock>,
    pub stop: AtomicBool,
    /// Set when the audio mixer should drop what it buffered (after a pause).
    pub flush_audio: AtomicBool,
    /// Clock time at which the first video frame arrived. Audio and events are aligned to it.
    first_frame_ms: AtomicU64,
    pub encoder: Mutex<Option<VideoEncoder>>,
    pub events: Mutex<Vec<InputEvent>>,
}

impl Shared {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            clock: Mutex::new(Clock::new(Instant::now())),
            stop: AtomicBool::new(false),
            flush_audio: AtomicBool::new(false),
            first_frame_ms: AtomicU64::new(NO_FRAME),
            encoder: Mutex::new(None),
            events: Mutex::new(Vec::new()),
        })
    }

    pub fn now_ms(&self) -> u64 {
        self.clock.lock().active_ms(Instant::now())
    }

    pub fn mark_first_frame(&self) {
        let now = self.now_ms();
        let _ = self.first_frame_ms.compare_exchange(NO_FRAME, now, Ordering::AcqRel, Ordering::Acquire);
    }

    pub fn first_frame(&self) -> Option<u64> {
        match self.first_frame_ms.load(Ordering::Acquire) {
            NO_FRAME => None,
            ms => Some(ms),
        }
    }
}

pub struct Recording {
    shared: Arc<Shared>,
    control: Option<CaptureControl<capture::Handler, capture::Error>>,
    audio: Option<audio::Audio>,
    input: Option<input::Input>,
    cfg: StartConfig,
    dirs: DataDirs,
    id: String,
    video_path: PathBuf,
    thumbnail_path: PathBuf,
    screen: Size,
    fps: u32,
}

impl Recording {
    pub fn start(cfg: StartConfig, dirs: DataDirs, levels: LevelsSink) -> Result<Self, String> {
        if matches!(cfg.mode, CaptureMode::Region | CaptureMode::Camera) {
            return Err("This capture mode is coming soon. Use Display or Window for now.".into());
        }

        let (date, time) = local_date_time();
        let folder = output_folder(&cfg.folder);
        std::fs::create_dir_all(&folder).map_err(|e| format!("Cannot create {}: {e}", folder.display()))?;
        dirs.ensure().map_err(|e| format!("Cannot prepare the app data folder: {e}"))?;
        let video_path = unique_path(&folder, &cfg.file_stem(&date, &time), "mp4");
        let id = format!("{:x}", now_ms());
        let thumbnail_path = dirs.thumbnail_file(&id);

        let shared = Shared::new();
        let started = capture::start(&cfg, &shared, &video_path, &thumbnail_path)?;

        let audio = (cfg.system || cfg.mic).then(|| audio::start(&cfg, shared.clone(), levels));
        let input = input::start(shared.clone(), started.origin);

        Ok(Self {
            shared,
            control: Some(started.control),
            audio,
            input: Some(input),
            fps: cfg.fps(),
            cfg,
            dirs,
            id,
            video_path,
            thumbnail_path,
            screen: started.screen,
        })
    }

    pub fn pause(&self) {
        self.shared.clock.lock().pause(Instant::now());
        if let Some(enc) = self.shared.encoder.lock().as_mut() {
            enc.pause();
        }
    }

    pub fn resume(&self) {
        // Drop sound buffered during the pause so it is not replayed after resuming.
        self.shared.flush_audio.store(true, Ordering::Release);
        self.shared.clock.lock().resume(Instant::now());
        if let Some(enc) = self.shared.encoder.lock().as_mut() {
            enc.resume();
        }
    }

    pub fn is_paused(&self) -> bool {
        self.shared.clock.lock().is_paused()
    }

    /// Length of the video so far.
    pub fn elapsed_ms(&self) -> u64 {
        self.shared.first_frame().map_or(0, |t0| self.shared.now_ms().saturating_sub(t0))
    }

    pub fn finish(mut self) -> Result<Entry, String> {
        let end_ms = self.shared.now_ms();
        self.shared.stop.store(true, Ordering::Release);

        if let Some(audio) = self.audio.take() {
            audio.join();
        }
        if let Some(input) = self.input.take() {
            input.stop();
        }
        if let Some(control) = self.control.take() {
            control.stop().map_err(|e| format!("Stopping capture failed: {e}"))?;
        }
        let encoder = self.shared.encoder.lock().take().ok_or("The recording never started.")?;
        encoder.finish().map_err(|e| format!("Saving the video failed: {e}"))?;

        let offset = self.shared.first_frame().ok_or("No video frames were captured.")?;
        let duration_ms = end_ms.saturating_sub(offset);
        let events = std::mem::take(&mut *self.shared.events.lock());

        let video_name = self.video_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // Only Studio recordings have editing data; Quick ones are just the video.
        let project_path = if self.cfg.studio {
            let project = Project::new(
                video_name,
                self.screen,
                self.fps,
                duration_ms,
                offset,
                true,
                true,
                self.cfg.system,
                self.cfg.mic,
                events,
            );
            let path = self.dirs.project_file(&self.id);
            project.save(&path).ok().map(|_| path.to_string_lossy().into_owned())
        } else {
            None
        };

        let size_mb = std::fs::metadata(&self.video_path).map(|m| m.len() as f64 / 1_048_576.0).unwrap_or(0.0);
        Ok(Entry {
            id: self.id.clone(),
            name: self.video_path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            video: self.video_path.to_string_lossy().into_owned(),
            thumbnail: self.thumbnail_path.exists().then(|| self.thumbnail_path.to_string_lossy().into_owned()),
            project: project_path,
            seconds: (duration_ms + 500) / 1000,
            size_mb,
            created_ms: now_ms(),
            studio: self.cfg.studio,
            screen: self.screen,
        })
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        // If a recording is abandoned, make sure no thread keeps running or hooks stay installed.
        self.shared.stop.store(true, Ordering::Release);
    }
}

pub fn local_date_time() -> (String, String) {
    let t = unsafe { GetLocalTime() };
    (
        format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay),
        format!("{:02}-{:02}-{:02}", t.wHour, t.wMinute, t.wSecond),
    )
}

fn unique_path(folder: &std::path::Path, stem: &str, ext: &str) -> PathBuf {
    let first = folder.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| folder.join(format!("{stem} ({n}).{ext}")))
        .find(|p| !p.exists())
        .expect("an unused file name exists")
}
