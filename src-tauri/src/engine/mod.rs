//! The recording engine. Windows has the real implementation (Graphics Capture, WASAPI, input
//! hooks); other platforms get a stub so the app still builds and the UI can be developed anywhere.

use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use velo_core::Levels;

#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::{list_sources, snapshot, Preview, Recording};

#[cfg(not(windows))]
mod stub;
#[cfg(not(windows))]
pub use stub::{list_sources, snapshot, Preview, Recording};

#[derive(Debug, Clone, Serialize)]
pub struct SourceInfo {
    pub name: String,
    pub detail: String,
}

/// Everything the UI can pick from. Indexes into these lists are what `StartConfig` carries.
#[derive(Debug, Clone, Serialize)]
pub struct Sources {
    pub displays: Vec<SourceInfo>,
    pub windows: Vec<SourceInfo>,
    pub mics: Vec<String>,
    pub speakers: Vec<String>,
}

pub type LevelsSink = Arc<dyn Fn(Levels) + Send + Sync>;
/// Receives preview frames as JPEG bytes.
pub type FrameSink = Arc<dyn Fn(Vec<u8>) + Send + Sync>;

/// Where recordings go. Relative paths are inside the user's profile; empty means `Videos\Velo`.
pub fn output_folder(configured: &str) -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let configured = configured.trim();
    if configured.is_empty() {
        return home.join("Videos").join("Velo");
    }
    let path = PathBuf::from(configured);
    if path.is_absolute() { path } else { home.join(path) }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_folder_resolves_relative_empty_and_absolute() {
        assert!(output_folder("").ends_with("Videos/Velo") || output_folder("").ends_with("Videos\\Velo"));
        assert!(output_folder("Videos\\Velo").to_string_lossy().ends_with("Velo"));
        #[cfg(unix)]
        assert_eq!(output_folder("/data/clips"), PathBuf::from("/data/clips"));
    }
}
