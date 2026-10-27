import { icon, esc, fmtShort, fmtSize, screenArt, select } from '../ui.js';
import { assetUrl } from '../bridge.js';
import { library, ui } from '../state.js';

const SORTS = ['Newest', 'Oldest', 'Longest', 'Largest'];

function sorted(list) {
  const by = {
    Newest: (a, b) => b.createdMs - a.createdMs,
    Oldest: (a, b) => a.createdMs - b.createdMs,
    Longest: (a, b) => b.seconds - a.seconds,
    Largest: (a, b) => b.sizeMb - a.sizeMb,
  }[ui.sort];
  return [...list].sort(by);
}

const when = (t) => new Date(t).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) +
  ', ' + new Date(t).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });

/** The real first frame when there is one, a neutral placeholder otherwise. Never a made-up screen. */
function picture(r) {
  const src = assetUrl(r.thumbnail);
  if (src) return `<img class="thumb" src="${esc(src)}" alt="" loading="lazy" />`;
  return `<div class="thumb none">${icon('film')}</div>`;
}

function card(r) {
  const badge = r.missing ? 'Missing' : r.studio ? 'Studio' : 'Quick';
  return `
    <article class="card rec-card ${r.missing ? 'is-missing' : ''}">
      <div class="rec-pic">${picture(r)}<span class="dur">${fmtShort(r.seconds)}</span><span class="tag">${badge}</span></div>
      <div><h3 title="${esc(r.name)}">${esc(r.name)}</h3><div class="sub">${when(r.createdMs)} &middot; ${fmtSize(r.sizeMb)}</div></div>
      <div class="acts">
        ${r.studio && !r.missing ? `<button class="btn sm primary" data-act="edit" data-id="${r.id}">${icon('pencil')}Edit</button>` : ''}
        <button class="btn sm" data-act="reveal" data-id="${r.id}">${icon('folder-open')}Show</button>
        <button class="btn sm" data-act="delete" data-id="${r.id}" aria-label="Remove from library" title="Remove from library">${icon('trash-2')}</button>
      </div>
    </article>`;
}

export function view() {
  const q = ui.search.trim().toLowerCase();
  const list = sorted(library.filter((r) => r.name.toLowerCase().includes(q)));

  const body = list.length
    ? `<div class="grid library">${list.map(card).join('')}</div>`
    : `<section class="card"><div class="empty">${icon('film')}
        <b>${library.length ? 'No recordings match your search' : 'No recordings yet'}</b>
        <span>${library.length ? 'Try a different name.' : 'Your finished recordings will show up here.'}</span>
        ${library.length ? '' : `<button class="btn primary" data-go="record">${icon('circle-dot')}Start recording</button>`}
      </div></section>`;

  return `
    <div class="page-head">
      <div class="page-title">
        <span class="chip-lg">${icon('library')}</span>
        <div><h1>Library</h1><p>${library.length} recording${library.length === 1 ? '' : 's'} saved on this PC.</p></div>
      </div>
    </div>
    <div class="toolbar">
      <label class="search">${icon('search')}<input class="field" type="search" placeholder="Search recordings" value="${esc(ui.search)}" aria-label="Search recordings" data-search /></label>
      ${select('sort', SORTS, SORTS.indexOf(ui.sort), 'Sort by')}
    </div>
    ${body}`;
}
