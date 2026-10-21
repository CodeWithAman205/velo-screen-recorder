import { icon, hydrate, html } from './ui.js';
import { invoke } from './bridge.js';

const state = {
  source: 'screen',
  system: true,
  mic: true,
  cursor: true,
  quality: '1080p',
  fps: '60',
  recording: false,
  paused: false,
  seconds: 0,
  timerId: null,
  recent: [],
};

const SOURCES = [
  { id: 'screen', icon: 'monitor', title: 'Full screen', sub: 'Entire display' },
  { id: 'window', icon: 'app-window', title: 'Window', sub: 'Pick an app' },
  { id: 'region', icon: 'crop', title: 'Region', sub: 'Drag an area' },
];

const fmt = (s) => [Math.floor(s / 3600), Math.floor(s / 60) % 60, s % 60].map((n) => String(n).padStart(2, '0')).join(':');

function toggleRow(key, ic, title, sub) {
  return html`<div class="row">
    <div class="card-title"><span class="chip">${icon(ic)}</span><div><b>${title}</b><small>${sub}</small></div></div>
    <button class="switch" role="switch" aria-label="${title}" aria-checked="${state[key]}" data-toggle="${key}"></button>
  </div>`;
}

function seg(key, options) {
  return html`<div class="seg" role="group">${options
    .map((o) => `<button aria-pressed="${state[key] === o}" data-seg="${key}" data-val="${o}">${o}</button>`)
    .join('')}</div>`;
}

function view() {
  const rec = state.recording;
  return html`
    <div class="page-head">
      <div class="page-title">
        <span class="chip-lg">${icon('video')}</span>
        <div><h1>Screen Recorder</h1><p>Record your screen with system sound and microphone, then save a clean MP4.</p></div>
      </div>
      <span class="pill">${icon('keyboard')} Alt + Shift + R</span>
    </div>

    <div class="grid two">
      <div class="stack">
        <section class="card ${rec ? 'recording' : ''}">
          <div class="card-head">
            <div class="card-title"><span class="chip">${icon('circle-dot')}</span>Capture</div>
            <span class="muted">${state.quality} &middot; ${state.fps} fps</span>
          </div>
          <div class="preview">
            <div>${icon(rec ? 'circle-dot' : 'monitor')}<div>${rec ? 'Recording in progress' : 'Preview appears here once recording starts'}</div></div>
          </div>
          <div class="rec-bar">
            <div class="timer"><span class="rec-dot"></span><span id="timer">${fmt(state.seconds)}</span></div>
            <div style="display:flex;gap:10px;flex-wrap:wrap">
              ${rec ? `<button class="btn" id="pause">${icon(state.paused ? 'play' : 'pause')}${state.paused ? 'Resume' : 'Pause'}</button>` : ''}
              ${rec
                ? `<button class="btn danger lg" id="stop">${icon('square')}Stop</button>`
                : `<button class="btn primary lg" id="start">${icon('circle-dot')}Start recording</button>`}
            </div>
          </div>
        </section>

        <section class="card">
          <div class="card-head"><div class="card-title"><span class="chip">${icon('monitor')}</span>What to record</div></div>
          <div class="tiles">
            ${SOURCES.map((s) => `<button class="tile" aria-pressed="${state.source === s.id}" data-source="${s.id}">
              <span class="chip">${icon(s.icon)}</span><span><b>${s.title}</b><small>${s.sub}</small></span></button>`).join('')}
          </div>
        </section>
      </div>

      <div class="stack">
        <section class="card">
          <div class="card-head"><div class="card-title"><span class="chip">${icon('volume-2')}</span>Audio and cursor</div></div>
          ${toggleRow('system', 'volume-2', 'System sound', 'Everything your PC plays')}
          ${toggleRow('mic', 'mic', 'Microphone', 'Your voice')}
          ${toggleRow('cursor', 'mouse-pointer-click', 'Show cursor', 'Cursor and clicks in the video')}
        </section>

        <section class="card">
          <div class="card-head"><div class="card-title"><span class="chip">${icon('settings')}</span>Quality</div></div>
          <div class="row"><div><b>Resolution</b><small>Output size</small></div>${seg('quality', ['720p', '1080p', '1440p', '4K'])}</div>
          <div class="row"><div><b>Frame rate</b><small>Smoothness</small></div>${seg('fps', ['30', '60'])}</div>
        </section>

        <section class="card">
          <div class="card-head">
            <div class="card-title"><span class="chip">${icon('film')}</span>Recent recordings</div>
            <button class="link">Open folder ${icon('chevron-right')}</button>
          </div>
          ${state.recent.length
            ? state.recent.map((r) => `<div class="list-row"><span>${r.name}</span><span class="muted">${fmt(r.seconds)}</span></div>`).join('')
            : `<div class="empty">${icon('film')}<div>No recordings yet</div></div>`}
        </section>
      </div>
    </div>`;
}

export function mountRecorder(root) {
  const render = () => {
    root.innerHTML = view();
    hydrate(root);
  };

  const tick = () => {
    state.seconds += 1;
    const el = root.querySelector('#timer');
    if (el) el.textContent = fmt(state.seconds);
  };

  root.addEventListener('click', async (e) => {
    const t = e.target.closest('button');
    if (!t) return;
    if (t.dataset.source) state.source = t.dataset.source;
    else if (t.dataset.toggle) state[t.dataset.toggle] = !state[t.dataset.toggle];
    else if (t.dataset.seg) state[t.dataset.seg] = t.dataset.val;
    else if (t.id === 'start') {
      await invoke('start_recording', { source: state.source, system: state.system, mic: state.mic, cursor: state.cursor, quality: state.quality, fps: Number(state.fps) });
      Object.assign(state, { recording: true, paused: false, seconds: 0 });
      state.timerId = setInterval(() => !state.paused && tick(), 1000);
    } else if (t.id === 'pause') {
      state.paused = !state.paused;
      await invoke(state.paused ? 'pause_recording' : 'resume_recording');
    } else if (t.id === 'stop') {
      clearInterval(state.timerId);
      const res = await invoke('stop_recording');
      state.recent.unshift({ name: res?.path?.split(/[\\/]/).pop() ?? 'recording.mp4', seconds: state.seconds });
      state.recording = false;
    } else return;
    render();
  });

  render();
}
