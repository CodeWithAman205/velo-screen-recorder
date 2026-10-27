//! The list of recordings, kept by the app (not beside the videos) so people only ever see their
//! `.mp4` files. Editing data (`projects/`) and thumbnails live here too.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use velo_director::Size;

/// Where the app keeps its own files. Shared by the standalone app and anything that embeds the engine.
#[derive(Debug, Clone)]
pub struct DataDirs {
    root: PathBuf,
}

impl DataDirs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn library_file(&self) -> PathBuf {
        self.root.join("library.json")
    }

    pub fn project_file(&self, id: &str) -> PathBuf {
        self.root.join("projects").join(format!("{id}.json"))
    }

    pub fn thumbnail_file(&self, id: &str) -> PathBuf {
        self.root.join("thumbnails").join(format!("{id}.jpg"))
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.root.join("projects"))?;
        std::fs::create_dir_all(self.root.join("thumbnails"))
    }
}

/// One recording as the UI shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub name: String,
    /// Full path of the video.
    pub video: String,
    pub thumbnail: Option<String>,
    /// Editing data for Studio recordings; `None` for Quick ones.
    pub project: Option<String>,
    pub seconds: u64,
    pub size_mb: f64,
    pub created_ms: u64,
    pub studio: bool,
    pub screen: Size,
}

#[derive(Debug)]
pub struct Library {
    file: PathBuf,
    entries: Vec<Entry>,
}

impl Library {
    /// Opens the library, starting empty if the file is missing or unreadable.
    pub fn open(dirs: &DataDirs) -> Self {
        let file = dirs.library_file();
        let entries = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { file, entries }
    }

    /// Newest first.
    pub fn list(&self) -> Vec<Entry> {
        let mut all = self.entries.clone();
        all.sort_by_key(|e| std::cmp::Reverse(e.created_ms));
        all
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn add(&mut self, entry: Entry) -> std::io::Result<()> {
        self.entries.retain(|e| e.id != entry.id);
        self.entries.push(entry);
        self.save()
    }

    /// Removes the entry and returns it, so the caller can clean up its files.
    pub fn remove(&mut self, id: &str) -> std::io::Result<Option<Entry>> {
        let Some(i) = self.entries.iter().position(|e| e.id == id) else { return Ok(None) };
        let removed = self.entries.remove(i);
        self.save()?;
        Ok(Some(removed))
    }

    /// Points an entry at a video that was moved.
    pub fn relocate(&mut self, id: &str, new_video: &str) -> std::io::Result<bool> {
        let Some(e) = self.entries.iter_mut().find(|e| e.id == id) else { return Ok(false) };
        e.video = new_video.to_string();
        self.save()?;
        Ok(true)
    }

    fn save(&self) -> std::io::Result<()> {
        write_atomic(&self.file, &serde_json::to_vec(&self.entries).map_err(std::io::Error::other)?)
    }
}

/// Write then rename, so a crash never leaves a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dirs(tag: &str) -> DataDirs {
        let root = std::env::temp_dir().join(format!("velo-lib-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dirs = DataDirs::new(root);
        dirs.ensure().unwrap();
        dirs
    }

    fn entry(id: &str, created: u64) -> Entry {
        Entry {
            id: id.into(),
            name: format!("Clip {id}"),
            video: format!("C:\\Videos\\{id}.mp4"),
            thumbnail: None,
            project: Some(format!("{id}.json")),
            seconds: 10,
            size_mb: 12.5,
            created_ms: created,
            studio: true,
            screen: Size { w: 1920.0, h: 1080.0 },
        }
    }

    #[test]
    fn lists_newest_first_and_survives_a_restart() {
        let dirs = temp_dirs("order");
        let mut lib = Library::open(&dirs);
        lib.add(entry("a", 100)).unwrap();
        lib.add(entry("b", 300)).unwrap();
        lib.add(entry("c", 200)).unwrap();
        let ids: Vec<_> = lib.list().into_iter().map(|e| e.id).collect();
        assert_eq!(ids, ["b", "c", "a"]);

        let reopened = Library::open(&dirs);
        assert_eq!(reopened.list().len(), 3);
        assert_eq!(reopened.get("c").unwrap().name, "Clip c");
        std::fs::remove_dir_all(&dirs.root).unwrap();
    }

    #[test]
    fn adding_the_same_id_replaces_instead_of_duplicating() {
        let dirs = temp_dirs("dup");
        let mut lib = Library::open(&dirs);
        lib.add(entry("a", 1)).unwrap();
        let mut again = entry("a", 1);
        again.seconds = 99;
        lib.add(again).unwrap();
        assert_eq!(lib.list().len(), 1);
        assert_eq!(lib.get("a").unwrap().seconds, 99);
        std::fs::remove_dir_all(&dirs.root).unwrap();
    }

    #[test]
    fn remove_returns_the_entry_and_forgets_it() {
        let dirs = temp_dirs("rm");
        let mut lib = Library::open(&dirs);
        lib.add(entry("a", 1)).unwrap();
        assert_eq!(lib.remove("a").unwrap().unwrap().id, "a");
        assert!(lib.remove("a").unwrap().is_none());
        assert!(Library::open(&dirs).list().is_empty());
        std::fs::remove_dir_all(&dirs.root).unwrap();
    }

    #[test]
    fn a_corrupt_library_file_opens_empty_instead_of_failing() {
        let dirs = temp_dirs("bad");
        std::fs::write(dirs.library_file(), b"{ not json").unwrap();
        assert!(Library::open(&dirs).list().is_empty());
        std::fs::remove_dir_all(&dirs.root).unwrap();
    }

    #[test]
    fn relocate_updates_the_video_path() {
        let dirs = temp_dirs("mv");
        let mut lib = Library::open(&dirs);
        lib.add(entry("a", 1)).unwrap();
        assert!(lib.relocate("a", "D:\\Moved\\a.mp4").unwrap());
        assert!(!lib.relocate("missing", "x").unwrap());
        assert_eq!(Library::open(&dirs).get("a").unwrap().video, "D:\\Moved\\a.mp4");
        std::fs::remove_dir_all(&dirs.root).unwrap();
    }

    #[test]
    fn data_dir_paths_are_stable() {
        let d = DataDirs::new("/data");
        assert!(d.project_file("x").ends_with("projects/x.json"));
        assert!(d.thumbnail_file("x").ends_with("thumbnails/x.jpg"));
    }
}
