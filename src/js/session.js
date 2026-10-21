import { invoke } from './bridge.js';
import { settings, session, addRecording, emit } from './state.js';

let timer = null;
let countdownTimer = null;

const stamp = () => {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  return { date: `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`, time: `${p(d.getHours())}-${p(d.getMinutes())}-${p(d.getSeconds())}` };
};

async function begin() {
  await invoke('start_recording', {
    config: {
      mode: settings.mode, display: settings.display, window: settings.window,
      system: settings.system, mic: settings.mic, camera: settings.camera,
      cursor: settings.cursor, quality: settings.quality, fps: Number(settings.fps),
    },
  });
  Object.assign(session, { recording: true, paused: false, seconds: 0, counting: 0 });
  timer = setInterval(() => {
    if (!session.paused) { session.seconds += 1; emit('tick'); }
  }, 1000);
  emit('state');
}

export function start() {
  if (session.recording || session.counting) return;
  const n = Number(settings.countdown) || 0;
  if (!n) { begin(); return; }
  session.counting = n;
  emit('state');
  countdownTimer = setInterval(() => {
    session.counting -= 1;
    if (session.counting <= 0) { clearInterval(countdownTimer); session.counting = 0; begin(); } else emit('state');
  }, 1000);
}

export function cancelCountdown() {
  clearInterval(countdownTimer);
  session.counting = 0;
  emit('state');
}

export async function togglePause() {
  session.paused = !session.paused;
  await invoke(session.paused ? 'pause_recording' : 'resume_recording');
  emit('state');
}

export async function stop() {
  clearInterval(timer);
  const res = await invoke('stop_recording');
  const { date, time } = stamp();
  const seconds = session.seconds;
  addRecording({
    id: String(Date.now()),
    name: settings.pattern.replace('{date}', date).replace('{time}', time),
    path: res?.path ?? '',
    seconds,
    size: res?.size ?? Math.max(1, seconds * 0.9),
    mode: settings.mode,
    createdAt: Date.now(),
  });
  Object.assign(session, { recording: false, paused: false, seconds: 0 });
  emit('state');
}
