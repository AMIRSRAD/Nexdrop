// Nexdrop page script.
//
//  * A small "Download" button appears over videos and audio players (like
//    IDM's). With one candidate it sends it straight to Nexdrop; with several
//    it lists them.
//  * Clicking a link to a file the browser would just open in the tab (video,
//    audio, PDF) sends it to Nexdrop instead, unless Alt/Shift is held.
//
// Everything visible lives in a shadow root so the page's styles can't touch it.
(function () {
  if (window.__nexdropContent) return;
  window.__nexdropContent = true;

  // ── settings ──────────────────────────────────────────────────────────────
  let settings = { autoIntercept: true, interceptLinks: true, videoButton: true, fileTypes: '', ignoreSites: '' };
  const loadSettings = () =>
    chrome.runtime.sendMessage({ type: 'GET_SETTINGS' }, (s) => {
      if (!chrome.runtime.lastError && s) settings = s;
    });
  loadSettings();
  chrome.storage.onChanged.addListener((changes) => {
    if (changes.settings || changes.autoIntercept) loadSettings();
  });

  const words = (t) => String(t || '').toLowerCase().split(/[\s,;]+/).map((w) => w.replace(/^\./, '')).filter(Boolean);
  const ignored = (url) => {
    try {
      const host = new URL(url).hostname.toLowerCase();
      return words(settings.ignoreSites).some((e) => {
        const site = e.replace(/^\*\./, '');
        return host === site || host.endsWith('.' + site);
      });
    } catch (_) {
      return true;
    }
  };

  // ── link clicks ───────────────────────────────────────────────────────────
  // Types a browser shows in the tab instead of downloading (those it downloads
  // itself are caught by the downloads handler in the background script).
  const INLINE = new Set(['pdf', 'mp4', 'webm', 'mkv', 'avi', 'mov', 'm4v', 'flv', 'wmv', 'mpg', 'mpeg', 'mp3', 'wav', 'flac', 'ogg', 'opus', 'm4a', 'aac']);

  document.addEventListener(
    'click',
    (e) => {
      if (e.button !== 0 || e.ctrlKey || e.metaKey) return;
      const a = e.target instanceof Element ? e.target.closest('a[href]') : null;
      if (!a || a.dataset.nexdropSkip) return;
      let url;
      try {
        url = new URL(a.href);
      } catch (_) {
        return;
      }
      if (!/^https?:$/.test(url.protocol)) return;

      // Alt or Shift: the user wants the browser's own handling.
      if (e.altKey || e.shiftKey) {
        chrome.runtime.sendMessage({ type: 'SKIP_NEXT', url: url.href });
        return;
      }
      if (!settings.autoIntercept || !settings.interceptLinks || ignored(url.href)) return;

      const ext = (url.pathname.split('.').pop() || '').toLowerCase();
      if (!INLINE.has(ext) || !words(settings.fileTypes).includes(ext)) return;

      e.preventDefault();
      e.stopPropagation();
      chrome.runtime.sendMessage({ type: 'SEND_TO_NEXDROP', url: url.href, referer: location.href, filename: a.getAttribute('download') || undefined }, (reply) => {
        if (chrome.runtime.lastError || !reply || !reply.success) {
          // Nexdrop isn't running: open the link the way the browser would have.
          a.dataset.nexdropSkip = '1';
          a.click();
          setTimeout(() => delete a.dataset.nexdropSkip, 500);
        }
      });
    },
    true,
  );

  // ── the video button ──────────────────────────────────────────────────────
  const host = document.createElement('div');
  host.style.cssText = 'all:initial;position:fixed;top:0;left:0;width:0;height:0;z-index:2147483647;';
  const root = host.attachShadow({ mode: 'closed' });
  root.innerHTML = `
    <style>
      .btn { position: fixed; display: none; align-items: center; gap: 6px; height: 28px; padding: 0 11px 0 9px;
        background: #5b8cff; color: #fff; border: 0; border-radius: 7px; cursor: pointer; font: 600 12px/1 -apple-system, "Segoe UI", Roboto, sans-serif;
        box-shadow: 0 4px 16px rgba(0,0,0,.35); user-select: none; transition: background .12s; }
      .btn:hover { background: #7aa2ff; }
      .btn svg { width: 14px; height: 14px; }
      .menu { position: fixed; display: none; min-width: 230px; max-width: 340px; padding: 5px; background: #16181d; color: #ecedf1;
        border: 1px solid rgba(255,255,255,.1); border-radius: 10px; box-shadow: 0 12px 32px rgba(0,0,0,.5);
        font: 12.5px/1.3 -apple-system, "Segoe UI", Roboto, sans-serif; }
      .item { display: flex; align-items: center; gap: 8px; width: 100%; padding: 7px 9px; border: 0; background: none; color: inherit; text-align: left; border-radius: 7px; cursor: pointer; }
      .item:hover { background: rgba(255,255,255,.08); }
      .tag { flex: none; font-size: 10px; font-weight: 700; padding: 2px 5px; border-radius: 4px; background: rgba(91,140,255,.2); color: #9db9ff; }
      .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      .size { flex: none; color: #8b8f9b; font-variant-numeric: tabular-nums; }
    </style>
    <button class="btn" part="btn"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4v10"/><path d="m7.5 10 4.5 4.5 4.5-4.5"/><path d="M5 19h14"/></svg><span>Download</span></button>
    <div class="menu"></div>`;
  const btn = root.querySelector('.btn');
  const menu = root.querySelector('.menu');
  const label = btn.querySelector('span');
  const attach = () => {
    const fs = document.fullscreenElement;
    const parent = fs && fs.tagName !== 'VIDEO' && fs.tagName !== 'AUDIO' ? fs : document.documentElement || document.body;
    if (parent) parent.appendChild(host);
  };
  attach();

  let current = null; // the media element the button belongs to
  let hideTimer = 0;
  let overOverlay = false;
  btn.addEventListener('pointerenter', () => (overOverlay = true));
  btn.addEventListener('pointerleave', () => (overOverlay = false));
  menu.addEventListener('pointerenter', () => (overOverlay = true));
  menu.addEventListener('pointerleave', () => (overOverlay = false));

  const sizeText = (n) => (n > 0 ? (n >= 1073741824 ? (n / 1073741824).toFixed(1) + ' GB' : Math.max(1, Math.round(n / 1048576)) + ' MB') : '');
  const nameOf = (m) => {
    try {
      const u = new URL(m.url);
      return decodeURIComponent(u.pathname.split('/').filter(Boolean).slice(-2).join('/')) || u.hostname;
    } catch (_) {
      return m.url;
    }
  };
  const flash = (text, ms = 1800) => {
    label.textContent = text;
    setTimeout(() => (label.textContent = 'Download'), ms);
  };

  const place = () => {
    if (!current || !current.isConnected) return hide();
    const r = current.getBoundingClientRect();
    if (r.width < 160 || r.height < 90 || r.bottom < 0 || r.top > innerHeight) return hide();
    btn.style.display = 'flex';
    btn.style.top = `${Math.max(8, r.top + 10)}px`;
    btn.style.left = `${Math.min(innerWidth - 110, Math.max(8, r.right - 108))}px`;
  };

  const hide = () => {
    btn.style.display = 'none';
    menu.style.display = 'none';
    current = null;
  };

  const show = (el) => {
    clearTimeout(hideTimer);
    if (!settings.videoButton || !settings.autoIntercept) return;
    current = el;
    place();
  };

  const sendEntry = (entry) => {
    menu.style.display = 'none';
    chrome.runtime.sendMessage({ type: 'SEND_MEDIA', url: entry.url, page: location.href }, (reply) => {
      flash(reply && reply.success ? 'Sent to Nexdrop' : 'Nexdrop isn’t running');
    });
  };

  btn.addEventListener('click', (e) => {
    e.stopPropagation();
    e.preventDefault();
    const el = current;
    chrome.runtime.sendMessage({ type: 'GET_MEDIA' }, (reply) => {
      const media = !chrome.runtime.lastError && reply && reply.media ? reply.media : [];
      const direct = el && (el.currentSrc || el.src);
      const fallback = direct && /^https?:/i.test(direct) ? [{ kind: 'file', url: direct, size: 0 }] : [];
      const list = media.length ? media : fallback;

      if (list.length === 0) return flash('Play the video first', 2400);
      // One obvious choice (a single entry, or a stream and nothing big beside it): send it. Otherwise let the user pick.
      const bigFiles = list.filter((m) => m.kind === 'file' && m.size >= 5 * 1048576);
      const best = reply && reply.best;
      if (list.length === 1) return sendEntry(list[0]);
      if (best && best.kind === 'stream' && bigFiles.length === 0) return sendEntry(best);
      // Several candidates: let the user pick.
      menu.innerHTML = '';
      list.forEach((m) => {
        const item = document.createElement('button');
        item.className = 'item';
        item.innerHTML = '<span class="tag"></span><span class="name"></span><span class="size"></span>';
        item.children[0].textContent = m.kind === 'stream' ? (/\.mpd/i.test(m.url) ? 'DASH' : 'HLS') : (m.mime || 'file').split('/').pop().toUpperCase().slice(0, 5);
        item.children[1].textContent = nameOf(m);
        item.children[2].textContent = sizeText(m.size);
        item.addEventListener('click', (ev) => {
          ev.stopPropagation();
          sendEntry(m);
        });
        menu.appendChild(item);
      });
      const r = btn.getBoundingClientRect();
      menu.style.display = 'block';
      menu.style.top = `${r.bottom + 6}px`;
      menu.style.left = `${Math.max(8, Math.min(innerWidth - 350, r.right - 240))}px`;
    });
  });

  // Players cover their video with their own layers, so the pointer is never
  // "over the video" as far as events go; test the pointer against the video boxes instead.
  const media = new Set();
  const scan = () => document.querySelectorAll('video, audio').forEach((el) => media.add(el));
  scan();
  new MutationObserver(scan).observe(document.documentElement, { childList: true, subtree: true });

  let lastMove = 0;
  document.addEventListener(
    'pointermove',
    (e) => {
      const now = performance.now();
      if (now - lastMove < 90) return;
      lastMove = now;
      for (const el of media) {
        if (!el.isConnected) {
          media.delete(el);
          continue;
        }
        const r = el.getBoundingClientRect();
        if (e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom && r.width >= 160 && r.height >= 90) {
          return show(el);
        }
      }
      if (!overOverlay && current) {
        clearTimeout(hideTimer);
        hideTimer = setTimeout(() => !overOverlay && hide(), 900);
      }
    },
    { passive: true },
  );
  // Playing a video shows the button for a moment even if the pointer is elsewhere.
  document.addEventListener(
    'play',
    (e) => {
      if (e.target instanceof HTMLMediaElement) {
        media.add(e.target);
        show(e.target);
        hideTimer = setTimeout(() => !overOverlay && hide(), 3500);
      }
    },
    true,
  );
  addEventListener('scroll', () => current && place(), { passive: true, capture: true });
  addEventListener('resize', () => current && place());
  document.addEventListener('click', (e) => {
    if (!e.composedPath().includes(host)) menu.style.display = 'none';
  });
  document.addEventListener('fullscreenchange', attach);
})();
