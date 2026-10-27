import { icon, card, row, seg, switchBtn, range, screenArt, fmtShort } from '../ui.js';
import { assetUrl } from '../bridge.js';
import { library, ui, editor, BACKGROUNDS } from '../state.js';

export const canvasVars = () =>
  `--bg-fill:${BACKGROUNDS[editor.bg]};--pad:${editor.pad}px;--rad:${editor.radius}px;` +
  `--shadow-frame:${editor.shadow ? '0 24px 60px rgba(0,0,0,0.35)' : 'none'}`;

/** Applies slider changes straight to the preview without re-rendering the page. */
export function paintCanvas() {
  const el = document.querySelector('.canvas');
  if (el) el.style.cssText = canvasVars();
}

export function view() {
  const rec = library.find((r) => r.id === ui.editorId) ?? library[0];
  if (!rec) {
    return `
      <div class="page-head"><div class="page-title"><span class="chip-lg">${icon('clapperboard')}</span>
        <div><h1>Editor</h1><p>Trim, add a background and zoom in before you export.</p></div></div></div>
      <section class="card"><div class="empty">${icon('clapperboard')}<b>Nothing to edit yet</b>
        <span>Record something first, then open it here.</span>
        <button class="btn primary" data-go="record">${icon('circle-dot')}Start recording</button></div></section>`;
  }

  const total = Math.max(rec.seconds, 1);
  const [t0, t1] = editor.trim;
  const kept = Math.round(((t1 - t0) / 100) * total);

  const timeline = `
    <div class="timeline">
      <div class="track-label">CLIP</div>
      <div class="track clip"><div class="keep" style="left:${t0}%;width:${t1 - t0}%"></div></div>
      <div class="dual">
        <input type="range" min="0" max="100" value="${t0}" aria-label="Trim start" data-trim="0" />
        <input type="range" min="0" max="100" value="${t1}" aria-label="Trim end" data-trim="1" />
      </div>
      <div class="track-label">ZOOM</div>
      <div class="track">${editor.zooms.map((z) => `<div class="zoomseg" style="left:${z.start}%;width:${z.end - z.start}%">${icon('zoom-in')}${z.scale}x</div>`).join('')}</div>
    </div>`;

  const swatches = `<div class="swatches">${BACKGROUNDS.map((b, i) =>
    `<button class="swatch" style="background:${b}" aria-label="Background ${i + 1}" aria-pressed="${editor.bg === i}" data-set="bg" data-val="${i}" data-num></button>`).join('')}</div>`;

  return `
    <div class="page-head">
      <div class="page-title">
        <span class="chip-lg">${icon('clapperboard')}</span>
        <div><h1>Editor</h1><p>${rec.name} &middot; ${fmtShort(kept)} after trimming</p></div>
      </div>
      <button class="btn primary lg" data-act="export">${icon('download')}Export</button>
    </div>
    <div class="grid editor">
      <div class="stack">
        <section class="card">
          <div class="canvas" style="${canvasVars()}"><div class="frame">${assetUrl(rec.thumbnail) ? `<img class="feed-full" src="${assetUrl(rec.thumbnail)}" alt="" />` : screenArt({})}</div></div>
          <div class="transport">
            <button class="btn" data-act="play">${icon('play')}Play</button>
            <span class="muted">${fmtShort(0)} / ${fmtShort(rec.seconds)}</span>
          </div>
          ${timeline}
        </section>
      </div>
      <div class="stack">
        ${card({ ic: 'image', title: 'Background', body: swatches })}
        ${card({ ic: 'layers', title: 'Frame', body: [
          row({ title: 'Padding', ctl: `<div style="width:150px">${range('pad', 0, 120, editor.pad, 'Padding')}</div>` }),
          row({ title: 'Corner radius', ctl: `<div style="width:150px">${range('radius', 0, 40, editor.radius, 'Corner radius')}</div>` }),
          row({ title: 'Shadow', ctl: switchBtn('shadow', editor.shadow, 'Shadow') }),
        ].join('') })}
        ${card({ ic: 'mouse-pointer-click', title: 'Cursor', body: [
          row({ title: 'Cursor size', ctl: `<div style="width:150px">${range('cursorSize', 60, 220, editor.cursorSize, 'Cursor size')}</div>` }),
          row({ title: 'Click ring', ctl: switchBtn('ring', editor.ring, 'Click ring') }),
          row({ title: 'Smoothing', ctl: seg('smooth', ['Low', 'Medium', 'High'], editor.smooth) }),
        ].join('') })}
        ${card({ ic: 'download', title: 'Export', body: [
          row({ title: 'Format', ctl: seg('format', ['MP4', 'GIF'], editor.format) }),
          row({ title: 'Resolution', ctl: seg('res', ['720p', '1080p', '4K'], editor.res) }),
        ].join('') })}
      </div>
    </div>`;
}
