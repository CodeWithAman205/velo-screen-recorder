import { icon, hydrate, fmtTime } from './ui.js';
import { inTauri, invoke, listen, windowControl } from './bridge.js';
import { settings, set, toggle, resetSettings, session, library, setLibrary, removeRecording, editor, ui, subscribe, devices, setDevices, live } from './state.js';
import * as sess from './session.js';
import { connectBackend } from './session.js';
import { view as recordView } from './views/record.js';
import { view as libraryView } from './views/library.js';
import { view as editorView, paintCanvas } from './views/editor.js';
import { view as settingsView } from './views/settings.js';

const ROUTES = {
  record: { title: 'Record', view: recordView },
  library: { title: 'Library', view: libraryView },
  editor: { title: 'Editor', view: editorView },
  settings: { title: 'Settings', view: settingsView },
};
const EDITOR_KEYS = new Set(['bg', 'pad', 'radius', 'shadow', 'cursorSize', 'ring', 'smooth', 'format', 'res']);

const $ = (id) => document.getElementById(id);
const root = document.documentElement;
const page = $('page');
let route = 'record';

// ---------- Rendering ----------
function paintShell() {
  $('page-name').textContent = ROUTES[route].title;
  document.querySelectorAll('[data-route]').forEach((b) => {
    if (b.dataset.route === route) b.setAttribute('aria-current', 'page'); else b.removeAttribute('aria-current');
  });
  const lib = $('nav-library').querySelector('.count');
  if (lib) lib.textContent = library.length || '';
  $('save-path').textContent = settings.folder;
  $('rec-pill').innerHTML = session.recording
    ? `<span class="pill rec"><span class="dot"></span><span data-timer>${fmtTime(session.seconds)}</span>${session.paused ? ' &middot; Paused' : ''}</span>`
    : '';
  $('theme-btn').innerHTML = icon(root.dataset.theme === 'dark' ? 'sun' : 'moon');
}

function render() {
  const active = document.activeElement;
  const wasSearch = active?.matches?.('[data-search]');
  const caret = wasSearch ? active.selectionStart : 0;
  page.innerHTML = ROUTES[route].view();
  paintShell();
  syncPreview();
  if (wasSearch) { const el = page.querySelector('[data-search]'); el?.focus(); el?.setSelectionRange(caret, caret); }
}

function go(name) {
  route = ROUTES[name] ? name : 'record';
  if (route === 'library') loadLibrary();
  if (location.hash !== `#/${route}`) history.replaceState(null, '', `#/${route}`);
  render();
  $('page').parentElement.scrollTop = 0;
}

function applyTheme(theme) {
  root.dataset.theme = theme;
  set('theme', theme);
}

let toastTimer;
function toast(msg) {
  const el = $('toast');
  el.textContent = msg;
  el.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.remove('show'), 2600);
}
const SOON = 'Coming soon.';

// ---------- Input handling ----------
const assign = (key, value) => (EDITOR_KEYS.has(key) ? (editor[key] = value) : set(key, value));

const ACTIONS = {
  start: () => sess.start(),
  stop: () => sess.stop(),
  pause: () => sess.togglePause(),
  cancel: () => sess.cancelCountdown(),
  edit: (el) => { ui.editorId = el.dataset.id; go('editor'); },
  delete: async (el) => {
    await invoke('library_remove', { id: el.dataset.id, deleteVideo: false });
    removeRecording(el.dataset.id);
    render();
    toast('Removed from the library. The video file is still in your folder.');
  },
  reveal: (el) => { const r = library.find((x) => x.id === el.dataset.id); invoke('reveal_in_folder', { path: r?.video ?? '' }); },
  'reveal-folder': () => invoke('reveal_in_folder', { path: settings.folder }),
  reset: () => { resetSettings(); applyTheme(settings.theme); render(); },
  play: () => toast(SOON),
  export: () => toast(SOON),
  browse: () => toast(SOON),
};

document.addEventListener('click', (e) => {
  const el = e.target.closest('button');
  if (!el || el.disabled) return;
  if (el.dataset.route || el.dataset.go) return go(el.dataset.route || el.dataset.go);
  if (el.dataset.tab) { ui.settingsTab = el.dataset.tab; return render(); }
  if (el.dataset.act) { ACTIONS[el.dataset.act]?.(el); return; }
  if (el.dataset.toggle) {
    const key = el.dataset.toggle;
    if (EDITOR_KEYS.has(key)) editor[key] = !editor[key]; else toggle(key);
    return render();
  }
  if (el.dataset.set) {
    const key = el.dataset.set;
    let val = el.dataset.val;
    if (el.hasAttribute('data-num')) val = Number(val);
    if (key === 'countdown') val = val === 'Off' ? '0' : val;
    if (key === 'theme') { val = val.toLowerCase(); applyTheme(val); return render(); }
    assign(key, val);
    if (key === 'onClose') invoke('set_close_to_tray', { enabled: val === 'Hide to tray' });
    if (key === 'mode' && val === 'window') refreshSources();
    render();
  }
});

document.addEventListener('change', (e) => {
  const el = e.target;
  if (el.dataset.select) {
    if (el.dataset.select === 'sort') ui.sort = el.selectedOptions[0].text;
    else assign(el.dataset.select, el.dataset.select === 'encoder' ? el.selectedOptions[0].text : Number(el.value));
    render();
  }
});

document.addEventListener('input', (e) => {
  const el = e.target;
  if (el.dataset.search !== undefined) { ui.search = el.value; return render(); }
  if (el.dataset.text) return set(el.dataset.text, el.value);
  if (el.dataset.range) {
    const key = el.dataset.range;
    assign(key, Number(el.value));
    const out = document.querySelector(`[data-out="${key}"]`);
    if (out) out.textContent = `${el.value}%`;
    if (EDITOR_KEYS.has(key)) paintCanvas();
    return;
  }
  if (el.dataset.trim !== undefined) {
    const i = Number(el.dataset.trim);
    const v = Number(el.value);
    editor.trim[i] = i === 0 ? Math.min(v, editor.trim[1] - 2) : Math.max(v, editor.trim[0] + 2);
    el.value = editor.trim[i];
    const keep = document.querySelector('.track .keep');
    if (keep) { keep.style.left = `${editor.trim[0]}%`; keep.style.width = `${editor.trim[1] - editor.trim[0]}%`; }
  }
});

document.addEventListener('keydown', (e) => {
  if (e.altKey && e.shiftKey && e.code === 'KeyR') {
    e.preventDefault();
    if (session.recording) sess.stop(); else sess.start();
  }
});

$('theme-btn').addEventListener('click', () => { applyTheme(root.dataset.theme === 'dark' ? 'light' : 'dark'); render(); });
document.querySelectorAll('[data-win]').forEach((b) => b.addEventListener('click', () => windowControl(b.dataset.win)));
window.addEventListener('hashchange', () => go(location.hash.replace('#/', '')));

// ---------- Live updates ----------
subscribe((kind, payload) => {
  if (kind === 'error') {
    toast(payload);
  } else if (kind === 'tick') {
    document.querySelectorAll('[data-timer]').forEach((n) => { n.textContent = fmtTime(session.seconds); });
  } else {
    render();
  }
});

// Level meters. In the browser preview these are simulated; Tauri replaces them with real levels.
const levels = { mic: 0, system: 0 };
const real = { mic: 0, system: 0 };
setInterval(() => {
  document.querySelectorAll('[data-meter]').forEach((m) => {
    if (m.classList.contains('off')) { m.querySelectorAll('i').forEach((b) => { b.className = ''; }); return; }
    const id = m.dataset.meter;
    const target = inTauri ? Math.sqrt(real[id]) : Math.random() * (id === 'mic' ? 1 : 0.8);
    levels[id] = inTauri ? target : levels[id] * 0.6 + target * 0.4;
    const on = Math.round(levels[id] * 16 * 1.4);
    m.querySelectorAll('i').forEach((b, i) => { b.className = i < on ? (i > 13 ? 'max' : i > 10 ? 'hot' : 'on') : ''; });
  });
}, 120);

// ---------- Real devices and events (Tauri only) ----------
async function refreshSources() {
  const found = await invoke('list_sources');
  if (!found) return;
  setDevices(found);
  if (settings.display >= devices.displays.length) set('display', 0);
  if (settings.window >= devices.windows.length) set('window', 0);
  if (settings.micDevice >= devices.mics.length) set('micDevice', 0);
  render();
  // Real pictures of each screen (and window), delivered one by one as they are captured.
  invoke('source_thumbnails', { kind: 'display' });
  if (settings.mode === 'window') invoke('source_thumbnails', { kind: 'window' });
}

async function loadLibrary() {
  const items = await invoke('library_list');
  if (!items) return;
  setLibrary(items);
  if (route === 'library' || route === 'editor') render(); else paintShell();
}

// ---------- Live preview ----------
let previewKey = '';

/** Which screen or window to show live, or null when no preview should run. */
function wantedPreview() {
  if (!inTauri || route !== 'record' || document.hidden) return null;
  if (session.recording || session.counting || session.saving) return null;
  if (settings.mode === 'display' && devices.displays.length) return { mode: 'display', index: settings.display };
  if (settings.mode === 'window' && devices.windows.length) return { mode: 'window', index: settings.window };
  return null;
}

function syncPreview() {
  const want = wantedPreview();
  const key = want ? `${want.mode}:${want.index}` : '';
  if (key === previewKey) return;
  previewKey = key;
  live.frame = '';
  if (!want) { invoke('stop_preview'); return; }
  invoke('start_preview', want).catch((e) => { previewKey = ''; toast(String(e)); });
}

document.addEventListener('visibilitychange', syncPreview);

async function initBackend() {
  if (!inTauri) return;
  await connectBackend();
  await listen('levels', (l) => { real.mic = l.mic; real.system = l.system; });
  await listen('preview-frame', (b64) => {
    live.frame = b64;
    const img = document.querySelector('[data-feed="preview"]');
    if (img) img.src = `data:image/jpeg;base64,${b64}`; else if (route === 'record') render();
  });
  await listen('source-thumb', ({ kind, index, jpeg }) => {
    live.thumbs[kind][index] = jpeg;
    const img = document.querySelector(`[data-feed="thumb-${kind}-${index}"]`);
    if (img && jpeg) img.src = `data:image/jpeg;base64,${jpeg}`; else if (route === 'record') render();
  });
  invoke('set_close_to_tray', { enabled: settings.onClose === 'Hide to tray' });
  loadLibrary();
  refreshSources();
}

hydrate();
go(location.hash.replace('#/', '') || 'record');
initBackend();
