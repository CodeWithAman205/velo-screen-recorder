use super::{FrameSink, LevelsSink, SourceInfo, Sources};
use velo_core::library::DataDirs;
use velo_core::{CaptureMode, Entry, StartConfig};

const UNSUPPORTED: &str = "Recording is only available on Windows.";

pub fn list_sources() -> Sources {
    let s = |n: &str, d: &str| SourceInfo { name: n.into(), detail: d.into() };
    Sources {
        displays: vec![s("Display 1", "1920 x 1080 (main)")],
        windows: vec![s("Preview window", "demo")],
        mics: vec!["Default microphone".into()],
        speakers: vec!["Default speakers".into()],
    }
}

pub fn snapshot(_: CaptureMode, _: usize, _: u32) -> Result<Vec<u8>, String> {
    Err(UNSUPPORTED.into())
}

pub struct Preview;

impl Preview {
    pub fn start(_: CaptureMode, _: usize, _: FrameSink) -> Result<Self, String> {
        Err(UNSUPPORTED.into())
    }
}

pub struct Recording;

impl Recording {
    pub fn start(_: StartConfig, _: DataDirs, _: LevelsSink) -> Result<Self, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn pause(&self) {}
    pub fn resume(&self) {}
    pub fn is_paused(&self) -> bool {
        false
    }
    pub fn elapsed_ms(&self) -> u64 {
        0
    }
    pub fn finish(self) -> Result<Entry, String> {
        Err(UNSUPPORTED.into())
    }
}
