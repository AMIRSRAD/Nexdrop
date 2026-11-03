// Nexdrop page script.
//
//  * A small "Download" button appears over a video or audio player while the pointer is
//    on it (like IDM's), only when there is something to download, and never over
//    decorative clips. One candidate goes straight to Nexdrop; several are listed.
//  * Clicking a link to a file the browser would just open in the tab (video, audio, PDF)
//    sends it to Nexdrop instead, unless Alt/Shift is held.
//  * Collects the links of a selection, or the file links of the page, for the menus.
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

  // ── links for "download links in selection" and "download all files" ──────
  const MAX_LINKS = 2000;
  // Extensions that mean "this link is a file", beyond the ones the user takes over.
  const FILE_EXT = new Set(
    ('zip rar 7z tar gz tgz xz bz2 zst iso img dmg pkg exe msi msix deb rpm apk appimage bin torrent cab ' +
      'mp4 mkv webm avi mov flv wmv m4v mpg mpeg ts mp3 flac wav aac ogg opus m4a wma ' +
      'pdf epub mobi djvu doc docx xls xlsx ppt pptx odt ods odp rtf csv ' +
      'jpg jpeg png gif webp bmp tif tiff svg psd raw heic ' +
      'srt vtt ass sub ttf otf woff woff2 jar py').split(' '),
  );
  const httpUrl = (href) => {
    try {
      const u = new URL(href, location.href);
      if (!/^https?:$/.test(u.protocol)) return null;
      u.hash = '';
      return u.href;
    } catch (_) {
      return null;
    }
  };
  const extOf = (url) => {
    try {
      const last = new URL(url).pathname.split('/').pop() || '';
      const dot = last.lastIndexOf('.');
      return dot > 0 ? last.slice(dot + 1).toLowerCase() : '';
    } catch (_) {
      return '';
    }
  };
  const textOf = (a) => (a ? (a.getAttribute('download') || a.textContent || a.title || '').replace(/\s+/g, ' ').trim().slice(0, 140) : '');
  // A link whose text is a file name ("Setup-2.1.exe" on a download.php?id= link) is a file too.
  const textNamesFile = (a) => {
    const m = /\.([a-z0-9]{2,5})$/i.exec(textOf(a));
    return !!m && FILE_EXT.has(m[1].toLowerCase());
  };
  const isFileLink = (url, a) =>
    FILE_EXT.has(extOf(url)) || words(settings.fileTypes).includes(extOf(url)) || !!(a && (a.hasAttribute('download') || textNamesFile(a)));
  const here = () => location.href.split('#')[0];

  /** Adds a link once: `found` maps url → { url, text, file }. */
  const collect = (found, href, a, forceFile) => {
    const url = httpUrl(href);
    if (!url || url === here() || found.size >= MAX_LINKS) return;
    const prev = found.get(url);
    const text = textOf(a);
    if (prev) {
      if (!prev.text && text) prev.text = text;
      return;
    }
    found.set(url, { url, text, file: forceFile || isFileLink(url, a) });
  };

  /** Links inside the current selection: anchors it touches, plus plain-text addresses in it. */
  function selectedLinks() {
    const sel = window.getSelection();
    if (!sel || sel.isCollapsed || sel.rangeCount === 0) return [];
    const found = new Map();
    for (let i = 0; i < sel.rangeCount; i++) {
      const range = sel.getRangeAt(i);
      let scope = range.commonAncestorContainer;
      if (scope.nodeType !== Node.ELEMENT_NODE) scope = scope.parentElement;
      if (!scope) continue;
      // A selection inside one link's text.
      const inside = scope.closest && scope.closest('a[href]');
      if (inside) collect(found, inside.href, inside);
      scope.querySelectorAll('a[href]').forEach((a) => {
        try {
          if (range.intersectsNode(a)) collect(found, a.href, a);
        } catch (_) { /* detached */ }
      });
    }
    (sel.toString().match(/https?:\/\/[^\s<>"'`)\]]+/gi) || []).forEach((u) => collect(found, u.replace(/[.,;:!?]+$/, ''), null));
    return [...found.values()];
  }

  /** The page's file links (and media sources); every link only if no file link exists. */
  function pageLinks() {
    const found = new Map();
    document.querySelectorAll('a[href]').forEach((a) => collect(found, a.href, a));
    document.querySelectorAll('video[src], audio[src], source[src]').forEach((m) => {
      if (!/^blob:/i.test(m.src)) collect(found, m.src, null, true);
    });
    const all = [...found.values()];
    const files = all.filter((l) => l.file);
    return files.length ? files : all;
  }

  const countPageFiles = () => {
    let n = 0;
    const seen = new Set();
    for (const a of document.querySelectorAll('a[href]')) {
      const url = httpUrl(a.href);
      if (url && !seen.has(url) && isFileLink(url, a)) {
        seen.add(url);
        if (++n >= 999) break;
      }
    }
    return n;
  };

  chrome.runtime.onMessage.addListener((message, _sender, reply) => {
    if (message && message.type === 'GET_LINKS') {
      reply({ links: message.mode === 'selection' ? selectedLinks() : pageLinks() });
    }
  });

  // Tell the menu what this right-click can offer, before the menu opens (mousedown comes
  // first, so the menu is usually up to date when it appears).
  // (Sent every time: the menu is shared by all tabs and frames.)
  let lastReport = 0;
  const reportCounts = () => {
    const now = performance.now();
    if (now - lastReport < 50) return; // mousedown and contextmenu of the same click
    lastReport = now;
    const counts = { selection: selectedLinks().length, pageFiles: countPageFiles() };
    chrome.runtime.sendMessage({ type: 'MENU_COUNTS', ...counts }, () => void chrome.runtime.lastError);
  };
  document.addEventListener('mousedown', (e) => e.button === 2 && reportCounts(), true);
  document.addEventListener('contextmenu', reportCounts, true);

  // ── the video button ──────────────────────────────────────────────────────
  const host = document.createElement('div');
  host.style.cssText = 'all:initial;position:fixed;top:0;left:0;width:0;height:0;z-index:2147483647;';
  const root = host.attachShadow({ mode: 'closed' });
  root.innerHTML = `
    <style>
      .bar { position: fixed; display: none; align-items: stretch; height: 28px; border-radius: 7px; overflow: hidden;
        box-shadow: 0 4px 16px rgba(0,0,0,.35); font: 600 12px/1 -apple-system, "Segoe UI", Roboto, sans-serif; opacity: 0; transition: opacity .15s; }
      .bar.on { opacity: 1; }
      .btn, .x { border: 0; background: #5b8cff; color: #fff; cursor: pointer; display: flex; align-items: center; user-select: none; font: inherit; }
      .btn { gap: 6px; padding: 0 10px 0 9px; }
      .btn:hover, .x:hover { background: #7aa2ff; }
      .x { padding: 0 7px; border-left: 1px solid rgba(255,255,255,.25); font-size: 14px; font-weight: 500; }
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
    <div class="bar"><button class="btn" title="Download with Nexdrop"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4v10"/><path d="m7.5 10 4.5 4.5 4.5-4.5"/><path d="M5 19h14"/></svg><span>Download</span></button><button class="x" title="Hide on this page">×</button></div>
    <div class="menu"></div>`;
  const bar = root.querySelector('.bar');
  const btn = root.querySelector('.btn');
  const closeBtn = root.querySelector('.x');
  const menu = root.querySelector('.menu');
  const label = btn.querySelector('span');
  host.setAttribute('data-nexdrop', 'hidden');
  const attach = () => {
    const fs = document.fullscreenElement;
    const parent = fs && fs.tagName !== 'VIDEO' && fs.tagName !== 'AUDIO' ? fs : document.documentElement || document.body;
    if (parent && host.parentNode !== parent) parent.appendChild(host);
  };
  attach();

  let current = null; // the media element the button belongs to
  let hideTimer = 0;
  let overOverlay = false;
  let dismissed = false; // "×": not again on this page
  [bar, menu].forEach((el) => {
    el.addEventListener('pointerenter', () => (overOverlay = true));
    el.addEventListener('pointerleave', () => (overOverlay = false));
  });

  const sizeText = (n) => (n > 0 ? (n >= 1073741824 ? (n / 1073741824).toFixed(1) + ' GB' : Math.max(1, Math.round(n / 1048576)) + ' MB') : '');
  const nameOf = (m) => {
    try {
      const u = new URL(m.url);
      return decodeURIComponent(u.pathname.split('/').filter(Boolean).slice(-2).join('/')) || u.hostname;
    } catch (_) {
      return m.url;
    }
  };
  let flashTimer = 0;
  const flash = (text, ms = 1800) => {
    clearTimeout(flashTimer);
    label.textContent = text;
    flashTimer = setTimeout(() => (label.textContent = 'Download'), ms);
  };

  const MIN_W = 200;
  const MIN_H = 112;
  /** A video worth offering: big enough, and not a muted background loop or a short clip. */
  const worthy = (el) => {
    const r = el.getBoundingClientRect();
    if (r.width < MIN_W || r.height < MIN_H) return false;
    if (el.tagName === 'VIDEO' && el.muted && el.loop && !el.controls) return false;
    if (Number.isFinite(el.duration) && el.duration > 0 && el.duration < 20) return false;
    return true;
  };

  // Is there anything to download for this element? The tab's captured media, or a direct src.
  let mediaCache = { at: 0, list: [], best: null };
  const fetchMedia = () =>
    new Promise((resolve) => {
      if (performance.now() - mediaCache.at < 4000) return resolve(mediaCache);
      chrome.runtime.sendMessage({ type: 'GET_MEDIA' }, (reply) => {
        const list = !chrome.runtime.lastError && reply && reply.media ? reply.media : [];
        mediaCache = { at: performance.now(), list, best: reply && reply.best };
        resolve(mediaCache);
      });
    });
  const directSrc = (el) => {
    const s = el && (el.currentSrc || el.src);
    return s && /^https?:/i.test(s) ? s : '';
  };

  const place = () => {
    if (!current || !current.isConnected) return hide();
    const r = current.getBoundingClientRect();
    if (r.width < MIN_W || r.height < MIN_H || r.bottom < 0 || r.top > innerHeight) return hide();
    bar.style.display = 'flex';
    bar.style.top = `${Math.max(8, r.top + 10)}px`;
    bar.style.left = `${Math.min(innerWidth - 140, Math.max(8, r.right - 138))}px`;
    requestAnimationFrame(() => bar.classList.add('on'));
  };

  const hide = () => {
    clearTimeout(hideTimer);
    bar.classList.remove('on');
    bar.style.display = 'none';
    menu.style.display = 'none';
    current = null;
    host.setAttribute('data-nexdrop', 'hidden');
  };

  // Like a player's own controls: the button goes away when the pointer rests or leaves,
  // and comes back as soon as it moves over the video again. Never while a menu is open.
  const armHide = (ms) => {
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      if (overOverlay || menu.style.display === 'block') return armHide(1500);
      hide();
    }, ms);
  };

  let showSeq = 0;
  const show = async (el) => {
    if (dismissed || !settings.videoButton || !settings.autoIntercept || ignored(location.href)) return;
    if (current === el && bar.style.display === 'flex') return armHide(2500);
    const seq = ++showSeq;
    const { list } = await fetchMedia();
    if (seq !== showSeq) return;
    // Nothing captured and no usable address: no button (it would only say "play it first").
    if (!list.length && !directSrc(el)) return;
    attach();
    current = el;
    place();
    if (bar.style.display === 'flex') host.setAttribute('data-nexdrop', 'visible');
    armHide(2500);
  };

  const sendEntry = (entry) => {
    menu.style.display = 'none';
    armHide(2500);
    chrome.runtime.sendMessage({ type: 'SEND_MEDIA', url: entry.url, page: location.href }, (reply) => {
      flash(!chrome.runtime.lastError && reply && reply.success ? 'Sent to Nexdrop' : 'Nexdrop isn’t running');
    });
  };

  closeBtn.addEventListener('click', (e) => {
    e.stopPropagation();
    e.preventDefault();
    dismissed = true;
    hide();
  });

  btn.addEventListener('click', async (e) => {
    e.stopPropagation();
    e.preventDefault();
    const el = current;
    mediaCache.at = 0; // fresh list: the player may have loaded a better quality meanwhile
    const { list: media, best } = await fetchMedia();
    const direct = directSrc(el);
    const list = media.length ? media : direct ? [{ kind: 'file', url: direct, size: 0 }] : [];

    if (list.length === 0) return flash('Play the video first', 2400);
    // One obvious choice (a single entry, or a stream and nothing big beside it): send it.
    const bigFiles = list.filter((m) => m.kind === 'file' && m.size >= 5 * 1048576);
    if (list.length === 1) return sendEntry(list[0]);
    if (best && best.kind === 'stream' && bigFiles.length === 0) return sendEntry(best);
    if (bigFiles.length === 1 && !list.some((m) => m.kind === 'stream')) return sendEntry(bigFiles[0]);
    // Several real candidates: let the user pick, biggest first.
    menu.innerHTML = '';
    [...list]
      .sort((a, b) => (b.kind === 'stream') - (a.kind === 'stream') || (b.size || 0) - (a.size || 0))
      .forEach((m) => {
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
    const r = bar.getBoundingClientRect();
    menu.style.display = 'block';
    menu.style.top = `${r.bottom + 6}px`;
    menu.style.left = `${Math.max(8, Math.min(innerWidth - 350, r.right - 240))}px`;
  });

  // Players cover their video with their own layers, so the pointer is never "over the
  // video" as far as events go. Look at everything under the pointer instead (cheap, and
  // nothing has to watch the page for changes).
  const mediaUnder = (x, y) => {
    for (const el of document.elementsFromPoint(x, y)) {
      if (el === host) return null;
      if ((el.tagName === 'VIDEO' || el.tagName === 'AUDIO') && worthy(el)) return el;
    }
    return null;
  };

  let lastMove = 0;
  let lastX = -1;
  let lastY = -1;
  let trailing = 0;
  const pointerAt = (x, y) => {
    // Browsers sometimes repeat pointer events while the pointer rests (a repaint, a scroll).
    // Only real movement counts, otherwise the button would never go away.
    if (Math.abs(x - lastX) < 3 && Math.abs(y - lastY) < 3) return;
    lastX = x;
    lastY = y;
    lastMove = performance.now();
    const el = mediaUnder(x, y);
    if (el) return void show(el);
    if (!overOverlay && current) armHide(700);
  };
  document.addEventListener(
    'pointermove',
    (e) => {
      if (dismissed || e.pointerType === 'touch') return;
      const wait = 120 - (performance.now() - lastMove);
      clearTimeout(trailing);
      // At most every 120 ms, but the last position always counts (where the pointer stopped).
      if (wait > 0) {
        const { clientX, clientY } = e;
        trailing = setTimeout(() => pointerAt(clientX, clientY), wait);
        return;
      }
      pointerAt(e.clientX, e.clientY);
    },
    { passive: true },
  );
  addEventListener('scroll', () => current && bar.style.display === 'flex' && place(), { passive: true, capture: true });
  addEventListener('resize', () => current && place());
  document.addEventListener('click', (e) => {
    if (!e.composedPath().includes(host)) menu.style.display = 'none';
  });
  document.addEventListener('keydown', (e) => e.key === 'Escape' && menu.style.display === 'block' && (menu.style.display = 'none'));
  document.addEventListener('fullscreenchange', () => {
    attach();
    if (current) place();
  });
})();
