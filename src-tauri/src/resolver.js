// Runs in the page Nexdrop opens for "Find the download". It presses the page's own download
// button (waiting out a countdown while the button is disabled) and nothing else.
(() => {
  if (window.__nexdropResolver) return;
  window.__nexdropResolver = true;

  const POSITIVE = /download|دانلود|télécharg|telecharg|скачать|descargar|herunterladen|baixar|indir|下载|get (the )?file|save file/i;
  // Promotions and other buttons that sit beside real download buttons.
  const NEGATIVE = /\bapp\b|install|extension|plugin|premium|upgrade|vpn|android|\bios\b|iphone|ipad|\bapk\b|buy|subscribe|report|share|stream|watch|play|trial|software|player|scan|virus|\bads?\b|sign ?up|log ?in|register|torrent|magnet/i;
  const FILE_EXT = /\.(zip|rar|7z|tar|gz|iso|exe|msi|dmg|apk|pkg|deb|mp4|mkv|webm|avi|mov|mp3|flac|wav|pdf|epub|docx?|xlsx?|pptx?)(\?|#|$)/i;
  const MAX_CLICKS = 3;
  const GIVE_UP_MS = 90000;

  const t0 = Date.now();
  const clicked = new WeakSet();
  let clicks = 0;

  const visible = (el) => {
    const r = el.getBoundingClientRect();
    if (r.width < 8 || r.height < 8) return false;
    const s = getComputedStyle(el);
    return s.visibility !== 'hidden' && s.display !== 'none' && Number(s.opacity) > 0.05 && s.pointerEvents !== 'none';
  };
  const disabled = (el) =>
    el.disabled === true ||
    el.getAttribute('aria-disabled') === 'true' ||
    /\b(disabled|wait|loading|countdown)\b/i.test(el.className && el.className.baseVal === undefined ? String(el.className) : '');

  const label = (el) => (el.innerText || el.value || el.getAttribute('aria-label') || el.title || '').replace(/\s+/g, ' ').trim();

  const score = (el) => {
    const text = label(el);
    const id = el.id || '';
    const cls = el.className && el.className.baseVal === undefined ? String(el.className) : '';
    const href = el.getAttribute('href') || '';
    const hay = `${text} ${id} ${cls} ${el.getAttribute('data-action') || ''} ${el.getAttribute('name') || ''}`;
    if (text.length > 60 || NEGATIVE.test(text) || NEGATIVE.test(id)) return 0;
    let s = 0;
    if (POSITIVE.test(text)) s += 4;
    if (/^(free )?download( now| file)?$/i.test(text)) s += 3;
    if (POSITIVE.test(id)) s += 3;
    if (POSITIVE.test(cls)) s += 2;
    if (POSITIVE.test(hay) && s === 0) s += 1;
    if (el.hasAttribute('download')) s += 3;
    if (FILE_EXT.test(href)) s += 3;
    if (s === 0) return 0;
    const r = el.getBoundingClientRect();
    s += Math.min(2, (r.width * r.height) / 20000);
    return s;
  };

  const press = (el) => {
    el.scrollIntoView({ block: 'center' });
    for (const type of ['pointerdown', 'mousedown', 'pointerup', 'mouseup']) {
      el.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, view: window }));
    }
    el.click();
  };

  const tick = () => {
    if (clicks >= MAX_CLICKS || Date.now() - t0 > GIVE_UP_MS) return clearInterval(timer);
    if (document.readyState === 'loading') return;
    let best = null;
    let bestScore = 0;
    document.querySelectorAll('a, button, input[type=button], input[type=submit], [role=button], [onclick]').forEach((el) => {
      if (clicked.has(el) || disabled(el) || !visible(el)) return;
      const s = score(el);
      if (s > bestScore) {
        best = el;
        bestScore = s;
      }
    });
    if (best && bestScore >= 3) {
      clicked.add(best);
      clicks++;
      try {
        press(best);
      } catch (_) { /* the page decides what happens next */ }
    }
  };
  const timer = setInterval(tick, 900);
})();
