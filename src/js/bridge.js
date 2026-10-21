// Thin wrapper over the Rust side. In a plain browser (design preview, tests) it falls back to
// a mock so the UI can be exercised without Tauri.

const tauri = window.__TAURI__;

export const inTauri = Boolean(tauri?.core?.invoke);

const mock = {
  list_sources: async () => null,
  start_recording: async () => {},
  stop_recording: async () => ({ path: '', seconds: 0, sizeMb: 0 }),
  pause_recording: async () => {},
  resume_recording: async () => {},
  reveal_in_folder: async () => {},
  set_close_to_tray: async () => {},
};

export async function invoke(cmd, args) {
  if (inTauri) return tauri.core.invoke(cmd, args);
  return mock[cmd]?.(args);
}

/** Subscribe to a backend event. Returns a promise of an unsubscribe function. */
export async function listen(event, handler) {
  if (!inTauri) return () => {};
  return tauri.event.listen(event, (e) => handler(e.payload));
}

export function windowControl(action) {
  if (!inTauri) return;
  tauri.window.getCurrentWindow()[action]();
}
