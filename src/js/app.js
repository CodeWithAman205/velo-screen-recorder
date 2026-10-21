import { icon, hydrate } from './ui.js';
import { windowControl } from './bridge.js';
import { mountRecorder } from './recorder-page.js';

const NAV = [
  { label: 'MAIN', items: [{ icon: 'layout-dashboard', label: 'Home' }] },
  { label: 'WORK', items: [
    { icon: 'video', label: 'Screen Recorder', current: true },
    { icon: 'zap', label: 'Productivity' },
    { icon: 'workflow', label: 'Automation' },
    { icon: 'wand-sparkles', label: 'Smart Tools' },
  ] },
  { label: 'SYSTEM', items: [
    { icon: 'sparkles', label: 'Cleanup' },
    { icon: 'shield-check', label: 'Protection' },
    { icon: 'hard-drive', label: 'Storage' },
    { icon: 'boxes', label: 'Management' },
    { icon: 'wrench', label: 'System Tools' },
  ] },
];

document.getElementById('nav').innerHTML = NAV.map((g) => `
  <div class="nav-group">
    <div class="nav-label">${g.label}</div>
    ${g.items.map((i) => `<button class="nav-item" ${i.current ? 'aria-current="page"' : ''} data-icon="${i.icon}" data-label="${i.label}" data-chev></button>`).join('')}
  </div>`).join('');

document.getElementById('crumbs').innerHTML =
  `<span class="root">WORK</span>${icon('chevron-right', 'sep')}<span>Screen Recorder</span>`;

const root = document.documentElement;
const themeBtn = document.getElementById('theme-btn');
const paintTheme = () => { themeBtn.innerHTML = icon(root.dataset.theme === 'dark' ? 'sun' : 'palette'); };
themeBtn.addEventListener('click', () => {
  root.dataset.theme = root.dataset.theme === 'dark' ? 'light' : 'dark';
  try { localStorage.setItem('velo-theme', root.dataset.theme); } catch (_) {}
  paintTheme();
});
paintTheme();

document.querySelectorAll('[data-win]').forEach((b) => b.addEventListener('click', () => windowControl(b.dataset.win)));

hydrate();
mountRecorder(document.getElementById('page'));
