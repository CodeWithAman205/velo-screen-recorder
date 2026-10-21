//! The small floating bar shown while recording. It is its own always-on-top window and is
//! excluded from screen capture, so it never appears in the video.

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "pill";
const WIDTH: f64 = 330.0;
const HEIGHT: f64 = 56.0;

pub fn show(app: &AppHandle) {
    if app.get_webview_window(LABEL).is_some() {
        return;
    }
    let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("pill.html".into()))
        .title("Velo Recorder")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .shadow(false)
        .build();
    let Ok(window) = built else { return };

    // Top centre of the main display.
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let x = monitor.position().x as f64 + (monitor.size().width as f64 - WIDTH * scale) / 2.0;
        let y = monitor.position().y as f64 + 20.0 * scale;
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
    exclude_from_capture(&window);
}

pub fn close(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.close();
    }
}

#[cfg(windows)]
fn exclude_from_capture(window: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};
    // Tauri may use a different `windows` version, so pass the handle across as a plain pointer.
    if let Ok(hwnd) = window.hwnd() {
        let raw = hwnd.0;
        let _ = unsafe { SetWindowDisplayAffinity(HWND(raw), WDA_EXCLUDEFROMCAPTURE) };
    }
}

#[cfg(not(windows))]
fn exclude_from_capture(_: &tauri::WebviewWindow) {}
