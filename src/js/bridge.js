// Thin wrapper over the Rust commands. In a plain browser (design preview, tests)
// it falls back to a mock so the UI can be exercised without Tauri.

const tauri = window.__TAURI__;

export const inTauri = Boolean(tauri?.core?.invoke);

const mock = {
  start_recording: async () => ({ id: 'mock', startedAt: Date.now() }),
  stop_recording: async () => ({ path: 'C:\\Users\\you\\Videos\\Velo\\recording.mp4', seconds: 0 }),
  pause_recording: async () => {},
  resume_recording: async () => {},
};

export async function invoke(cmd, args) {
  if (inTauri) return tauri.core.invoke(cmd, args);
  return mock[cmd]?.(args);
}

export function windowControl(action) {
  if (!inTauri) return;
  tauri.window.getCurrentWindow()[action]();
}
