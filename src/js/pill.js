import { icon } from './ui.js';
import { invoke, listen } from './bridge.js';

const $ = (id) => document.getElementById(id);
let paused = false;

const fmt = (ms) => {
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  return m >= 60 ? `${Math.floor(m / 60)}:${String(m % 60).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}` : `${String(m).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
};

function paint() {
  $('bar').classList.toggle('paused', paused);
  $('pause').innerHTML = icon(paused ? 'play' : 'pause');
  $('pause').setAttribute('aria-label', paused ? 'Resume' : 'Pause');
  $('stop').innerHTML = icon('square');
}

$('pause').addEventListener('click', () => invoke(paused ? 'resume_recording' : 'pause_recording'));
$('stop').addEventListener('click', () => invoke('stop_recording'));

listen('rec-tick', ({ ms, paused: p }) => {
  paused = p;
  $('time').textContent = fmt(ms);
  paint();
});
listen('rec-state', ({ state }) => {
  if (state === 'paused') paused = true;
  if (state === 'recording') paused = false;
  paint();
});
listen('levels', ({ mic }) => { $('level').style.width = `${Math.min(100, Math.sqrt(mic) * 100)}%`; });

paint();
