mod engine;
mod pill;

use base64::Engine as _;
use engine::{Preview, Recording, Sources};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use velo_core::library::{DataDirs, Entry, Library};
use velo_core::{CaptureMode, Project, StartConfig};

struct AppState {
    dirs: DataDirs,
    library: Mutex<Library>,
    recording: Mutex<Option<Recording>>,
    preview: Mutex<Option<Preview>>,
    close_to_tray: AtomicBool,
    quitting: AtomicBool,
}

fn emit_state(app: &AppHandle, state: &str, extra: serde_json::Value) {
    let _ = app.emit("rec-state", json!({ "state": state, "data": extra }));
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Lets the UI load files from a folder (thumbnails, videos) through the asset protocol.
fn allow_dir(app: &AppHandle, dir: &Path) {
    let _ = app.asset_protocol_scope().allow_directory(dir, true);
}

// ---------- Sources and previews ----------

#[tauri::command(async)]
fn list_sources() -> Sources {
    engine::list_sources()
}

/// Fetches a small picture of every display or window, sending each as it arrives.
#[tauri::command(async)]
fn source_thumbnails(app: AppHandle, kind: String) {
    let (mode, count) = match kind.as_str() {
        "window" => (CaptureMode::Window, engine::list_sources().windows.len()),
        _ => (CaptureMode::Display, engine::list_sources().displays.len()),
    };
    std::thread::spawn(move || {
        for index in 0..count {
            let jpeg = engine::snapshot(mode, index, 360).ok().map(|b| b64(&b));
            let _ = app.emit("source-thumb", json!({ "kind": kind, "index": index, "jpeg": jpeg }));
        }
    });
}

#[tauri::command(async)]
fn start_preview(app: AppHandle, state: State<'_, AppState>, mode: CaptureMode, index: usize) -> Result<(), String> {
    stop_preview_inner(&state);
    if state.recording.lock().is_some() {
        return Ok(());
    }
    let sink: engine::FrameSink = Arc::new(move |jpeg| {
        let _ = app.emit("preview-frame", b64(&jpeg));
    });
    *state.preview.lock() = Some(Preview::start(mode, index, sink)?);
    Ok(())
}

fn stop_preview_inner(state: &AppState) {
    // Dropping the preview stops its capture.
    state.preview.lock().take();
}

#[tauri::command(async)]
fn stop_preview(state: State<'_, AppState>) {
    stop_preview_inner(&state);
}

// ---------- Recording ----------

#[tauri::command]
fn default_folder() -> String {
    "Videos\\Velo".into()
}

#[tauri::command(async)]
fn start_recording(app: AppHandle, state: State<'_, AppState>, config: StartConfig) -> Result<(), String> {
    // The preview and the recording would capture the same screen twice.
    stop_preview_inner(&state);

    let mut slot = state.recording.lock();
    if slot.is_some() {
        return Err("A recording is already running.".into());
    }

    let levels_app = app.clone();
    let levels = Arc::new(move |l: velo_core::Levels| {
        let _ = levels_app.emit("levels", json!({ "mic": l.mic, "system": l.system }));
    });
    let hide_window = config.hide_window;
    allow_dir(&app, &engine::output_folder(&config.folder));
    *slot = Some(Recording::start(config, state.dirs.clone(), levels)?);
    drop(slot);

    pill::show(&app);
    if hide_window {
        if let Some(main) = app.get_webview_window("main") {
            let _ = main.hide();
        }
    }
    emit_state(&app, "recording", json!({}));
    spawn_ticker(app);
    Ok(())
}

/// Reports elapsed time to the windows while a recording runs.
fn spawn_ticker(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(250));
        let state = app.state::<AppState>();
        let info = state.recording.lock().as_ref().map(|r| (r.elapsed_ms(), r.is_paused()));
        let Some((ms, paused)) = info else { return };
        let _ = app.emit("rec-tick", json!({ "ms": ms, "paused": paused }));
    });
}

fn stop_inner(app: &AppHandle) -> Result<Entry, String> {
    let state = app.state::<AppState>();
    let recording = state.recording.lock().take().ok_or("Nothing is being recorded.")?;
    pill::close(app);
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    }
    emit_state(app, "saving", json!({}));
    let result = recording.finish().and_then(|entry| {
        state.library.lock().add(entry.clone()).map_err(|e| format!("Saved the video but not the library entry: {e}"))?;
        Ok(entry)
    });
    match &result {
        Ok(entry) => emit_state(app, "stopped", json!(entry)),
        Err(e) => emit_state(app, "failed", json!({ "message": e })),
    }
    result
}

#[tauri::command(async)]
fn stop_recording(app: AppHandle) -> Result<Entry, String> {
    stop_inner(&app)
}

#[tauri::command]
fn pause_recording(app: AppHandle, state: State<'_, AppState>) {
    if let Some(r) = state.recording.lock().as_ref() {
        r.pause();
        emit_state(&app, "paused", json!({}));
    }
}

#[tauri::command]
fn resume_recording(app: AppHandle, state: State<'_, AppState>) {
    if let Some(r) = state.recording.lock().as_ref() {
        r.resume();
        emit_state(&app, "recording", json!({}));
    }
}

fn toggle_pause(app: &AppHandle) {
    let state = app.state::<AppState>();
    let paused = state.recording.lock().as_ref().map(Recording::is_paused);
    match paused {
        Some(true) => resume_recording(app.clone(), app.state()),
        Some(false) => pause_recording(app.clone(), app.state()),
        None => {}
    }
}

// ---------- Library ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LibraryItem {
    #[serde(flatten)]
    entry: Entry,
    /// The video is no longer at the path the library remembers.
    missing: bool,
}

#[tauri::command]
fn library_list(app: AppHandle, state: State<'_, AppState>) -> Vec<LibraryItem> {
    let entries = state.library.lock().list();
    for dir in entries.iter().filter_map(|e| Path::new(&e.video).parent()) {
        allow_dir(&app, dir);
    }
    entries.into_iter().map(|entry| LibraryItem { missing: !Path::new(&entry.video).exists(), entry }).collect()
}

#[tauri::command(async)]
fn library_remove(state: State<'_, AppState>, id: String, delete_video: bool) -> Result<(), String> {
    let removed = state.library.lock().remove(&id).map_err(|e| e.to_string())?;
    if let Some(entry) = removed {
        // The app's own files always go; the user's video only if they asked.
        for file in [entry.thumbnail.as_deref(), entry.project.as_deref()].into_iter().flatten() {
            let _ = std::fs::remove_file(file);
        }
        if delete_video {
            let _ = std::fs::remove_file(&entry.video);
        }
    }
    Ok(())
}

#[tauri::command]
fn library_relocate(app: AppHandle, state: State<'_, AppState>, id: String, path: String) -> Result<bool, String> {
    if let Some(dir) = Path::new(&path).parent() {
        allow_dir(&app, dir);
    }
    state.library.lock().relocate(&id, &path).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn load_project(state: State<'_, AppState>, id: String) -> Result<Project, String> {
    let entry = state.library.lock().get(&id).cloned().ok_or("That recording is not in the library.")?;
    let path = entry.project.ok_or("This recording was made in Quick mode, so it has nothing to edit.")?;
    Project::load(Path::new(&path)).map_err(|e| e.to_string())
}

#[tauri::command]
fn reveal_in_folder(path: String) {
    #[cfg(windows)]
    {
        // The UI sends the save folder as typed (often relative to the user profile).
        let resolved = if Path::new(&path).is_absolute() { std::path::PathBuf::from(&path) } else { engine::output_folder(&path) };
        let p = resolved.as_path();
        let mut cmd = std::process::Command::new("explorer");
        if p.is_file() {
            cmd.arg(format!("/select,{}", p.display()));
        } else if p.is_dir() {
            cmd.arg(p);
        } else if let Some(parent) = p.parent().filter(|d| d.is_dir()) {
            cmd.arg(parent);
        }
        let _ = cmd.spawn();
    }
    #[cfg(not(windows))]
    let _ = path;
}

#[tauri::command]
fn set_close_to_tray(state: State<'_, AppState>, enabled: bool) {
    state.close_to_tray.store(enabled, Ordering::Relaxed);
}

// ---------- App ----------

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn quit(app: &AppHandle) {
    app.state::<AppState>().quitting.store(true, Ordering::Relaxed);
    if app.state::<AppState>().recording.lock().is_some() {
        let _ = stop_inner(app);
    }
    app.exit(0);
}

pub fn run() {
    let start_key = Shortcut::new(Some(Modifiers::ALT | Modifiers::SHIFT), Code::KeyR);
    let pause_key = Shortcut::new(Some(Modifiers::ALT | Modifiers::SHIFT), Code::KeyP);

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    let app = app.clone();
                    if *shortcut == start_key {
                        // Stopping needs no settings; starting does, so the window with the settings begins it.
                        if app.state::<AppState>().recording.lock().is_some() {
                            std::thread::spawn(move || {
                                let _ = stop_inner(&app);
                            });
                        } else {
                            let _ = app.emit_to("main", "hotkey-start", ());
                        }
                    } else if *shortcut == pause_key {
                        toggle_pause(&app);
                    }
                })
                .build(),
        )
        .setup(move |app| {
            let data_root = app.path().app_data_dir()?;
            let dirs = DataDirs::new(data_root.clone());
            let _ = dirs.ensure();
            allow_dir(app.handle(), &data_root);
            let library = Library::open(&dirs);
            app.manage(AppState {
                dirs,
                library: Mutex::new(library),
                recording: Mutex::new(None),
                preview: Mutex::new(None),
                close_to_tray: AtomicBool::new(true),
                quitting: AtomicBool::new(false),
            });

            // The recorder's own window should never show up in a recording, or in its own preview.
            if let Some(main) = app.get_webview_window("main") {
                pill::exclude_from_capture(&main);
            }

            // Another app may already own a shortcut; the recorder still works from its buttons then.
            let _ = app.global_shortcut().register(start_key);
            let _ = app.global_shortcut().register(pause_key);

            let show = MenuItem::with_id(app, "show", "Show Velo", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit_item])?;
            let mut tray = TrayIconBuilder::new()
                .tooltip("Velo Recorder")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "quit" => quit(app),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        show_main(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.app_handle().state::<AppState>();
                if state.close_to_tray.load(Ordering::Relaxed) && !state.quitting.load(Ordering::Relaxed) {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    quit(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_sources,
            source_thumbnails,
            start_preview,
            stop_preview,
            default_folder,
            start_recording,
            stop_recording,
            pause_recording,
            resume_recording,
            library_list,
            library_remove,
            library_relocate,
            load_project,
            reveal_in_folder,
            set_close_to_tray,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Velo Recorder");
}
