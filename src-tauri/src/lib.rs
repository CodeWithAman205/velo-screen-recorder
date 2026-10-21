mod engine;
mod pill;

use engine::{Finished, Recording, Sources};
use parking_lot::Mutex;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use velo_core::{Project, StartConfig};

#[derive(Default)]
struct AppState {
    recording: Mutex<Option<Recording>>,
    close_to_tray: AtomicBool,
    quitting: AtomicBool,
}

fn emit_state(app: &AppHandle, state: &str, extra: serde_json::Value) {
    let _ = app.emit("rec-state", json!({ "state": state, "data": extra }));
}

// ---------- Commands ----------

#[tauri::command(async)]
fn list_sources() -> Sources {
    engine::list_sources()
}

#[tauri::command]
fn default_folder() -> String {
    "Videos\\Velo".into()
}

#[tauri::command(async)]
fn start_recording(app: AppHandle, state: State<'_, AppState>, config: StartConfig) -> Result<(), String> {
    let mut slot = state.recording.lock();
    if slot.is_some() {
        return Err("A recording is already running.".into());
    }

    let levels_app = app.clone();
    let levels = Arc::new(move |l: velo_core::Levels| {
        let _ = levels_app.emit("levels", json!({ "mic": l.mic, "system": l.system }));
    });
    let hide_window = config.hide_window;
    *slot = Some(Recording::start(config, levels)?);
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

fn stop_inner(app: &AppHandle) -> Result<Finished, String> {
    let recording = app.state::<AppState>().recording.lock().take().ok_or("Nothing is being recorded.")?;
    pill::close(app);
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    }
    emit_state(app, "saving", json!({}));
    match recording.finish() {
        Ok(done) => {
            emit_state(app, "stopped", json!(done));
            Ok(done)
        }
        Err(e) => {
            emit_state(app, "failed", json!({ "message": e }));
            Err(e)
        }
    }
}

#[tauri::command(async)]
fn stop_recording(app: AppHandle) -> Result<Finished, String> {
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

#[tauri::command]
fn reveal_in_folder(path: String) {
    #[cfg(windows)]
    {
        let p = std::path::Path::new(&path);
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

#[tauri::command(async)]
fn load_project(path: String) -> Result<Project, String> {
    Project::load(std::path::Path::new(&path)).map_err(|e| e.to_string())
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
        .manage(AppState { close_to_tray: AtomicBool::new(true), ..Default::default() })
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
            default_folder,
            start_recording,
            stop_recording,
            pause_recording,
            resume_recording,
            reveal_in_folder,
            load_project,
            set_close_to_tray,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Velo Recorder");
}
