import { ICONS } from './icons.js';

/** Inline Lucide icon as an SVG string. */
export function icon(name, cls = '') {
  const body = ICONS[name];
  if (!body) throw new Error(`Unknown icon: ${name}`);
  return `<svg class="icon ${cls}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;
}

/** Replace every [data-icon] placeholder and fill [data-label] buttons. */
export function hydrate(root = document) {
  root.querySelectorAll('[data-icon]').forEach((el) => {
    const label = el.dataset.label;
    const chev = el.hasAttribute('data-chev');
    if (label !== undefined) {
      el.innerHTML = `${icon(el.dataset.icon)}<span class="grow">${label}</span>${chev ? icon('chevron-right', 'chev') : ''}`;
    } else {
      el.innerHTML = icon(el.dataset.icon);
    }
    el.removeAttribute('data-icon');
  });
}

export const html = (strings, ...vals) => strings.reduce((a, s, i) => a + s + (vals[i] ?? ''), '');
