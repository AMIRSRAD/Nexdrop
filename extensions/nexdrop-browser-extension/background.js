// Nexdrop browser integration: background logic (Chrome, Edge, Firefox).
//
// What it does:
//  * hands downloads the browser starts over to Nexdrop (with the page's
//    cookies, so files behind a login work), unless the site is ignored or
//    the user held Alt;
//  * notes the videos and media files each tab loads (HLS/DASH playlists and
//    direct video/audio files), so the page button and the popup can offer them;
//  * serves the content script, popup and options page.
//
// Runs as a service worker in Chrome/Edge and as an event page in Firefox.

const NEXDROP_SERVER = 'http://127.0.0.1:18888';

const DEFAULTS = {
  autoIntercept: true, // take over downloads the browser starts
  interceptLinks: true, // take over clicks on videos / PDFs / audio that browsers would open in the tab
  videoButton: true, // the "Download" button over videos
  fileTypes:
    'zip rar 7z tar gz tgz xz bz2 zst iso img dmg pkg exe msi msix deb rpm apk appimage bin torrent ' +
    'mp4 mkv webm avi mov flv wmv m4v mpg mpeg mp3 flac wav aac ogg opus m4a ' +
    'pdf epub doc docx xls xlsx ppt pptx',
  ignoreSites: '',
  minSizeMB: 10, // downloads of unknown type are taken over from this size up
};

// ── settings ─────────────────────────────────────────────────────────────────

let settingsCache = null;

async function getSettings() {
  if (settingsCache) return settingsCache;
  const stored = await chrome.storage.local.get(['settings', 'autoIntercept']);
  settingsCache = { ...DEFAULTS, ...(stored.settings || {}) };
  // The popup used to keep this switch on its own.
  if (stored.settings == null && stored.autoIntercept === false) settingsCache.autoIntercept = false;
  return settingsCache;
}

chrome.storage.onChanged.addListener((changes) => {
  if (changes.settings || changes.autoIntercept) settingsCache = null;
});

const words = (text) =>
  String(text || '')
    .toLowerCase()
    .split(/[\s,;]+/)
    .map((w) => w.replace(/^\./, ''))
    .filter(Boolean);

function isIgnoredSite(url, ignoreSites) {
  let host;
  try {
    host = new URL(url).hostname.toLowerCase();
  } catch (_) {
    return true;
  }
  return words(ignoreSites).some((entry) => {
    const site = entry.replace(/^\*\./, '');
    return host === site || host.endsWith('.' + site);
  });
}

function extensionOf(nameOrUrl) {
  let path = String(nameOrUrl || '');
  try {
    if (/^https?:/i.test(path)) path = new URL(path).pathname;
  } catch (_) { /* keep as is */ }
  const base = path.split(/[\\/]/).pop() || '';
  const dot = base.lastIndexOf('.');
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : '';
}

// ── bridge ───────────────────────────────────────────────────────────────────

async function cookieHeaderFor(url) {
  try {
    const cookies = await chrome.cookies.getAll({ url });
    return cookies.length ? cookies.map((c) => `${c.name}=${c.value}`).join('; ') : undefined;
  } catch (_) {
    return undefined; // no permission, or a URL the cookie store doesn't know
  }
}

/** Sends a link to the app. Resolves true when Nexdrop took it. */
async function sendToNexdrop(url, referer, filename, extra = {}) {
  try {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 6000);
    const res = await fetch(`${NEXDROP_SERVER}/download`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      signal: controller.signal,
      body: JSON.stringify({
        url,
        referer: referer || undefined,
        filename: filename || undefined,
        user_agent: navigator.userAgent,
        cookies: await cookieHeaderFor(url),
        ...extra,
      }),
    });
    clearTimeout(timer);
    return res.ok;
  } catch (err) {
    console.warn('Nexdrop is not reachable:', err && err.message);
    return false;
  }
}

// ── telling the app this browser is connected ──────────────────────────────

async function browserName() {
  const ua = navigator.userAgent;
  if (/Firefox\//.test(ua)) return 'firefox';
  if (/Edg\//.test(ua)) return 'edge';
  try {
    if (navigator.brave && (await navigator.brave.isBrave())) return 'brave';
  } catch (_) { /* not Brave */ }
  return 'chrome';
}

async function sayHello() {
  try {
    await fetch(`${NEXDROP_SERVER}/hello`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ browser: await browserName(), version: chrome.runtime.getManifest().version }),
    });
  } catch (_) { /* the app isn't running; try again later */ }
}

sayHello();
chrome.runtime.onStartup.addListener(sayHello);
chrome.runtime.onInstalled.addListener(sayHello);
// Browsers stop idle background scripts; an alarm wakes this one now and then.
chrome.alarms.get('nexdrop-hello', (alarm) => {
  if (!alarm) chrome.alarms.create('nexdrop-hello', { periodInMinutes: 5 });
});
chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === 'nexdrop-hello') sayHello();
});

// ── deciding which browser downloads to take over ────────────────────────────

const recentlySent = new Map(); // url → time, to ignore the browser announcing a download twice
const skipOnce = new Map(); // url → expiry: the user held Alt on this link

function remember(map, url, ttl) {
  const now = Date.now();
  for (const [k, t] of map) if (t < now) map.delete(k);
  map.set(url, now + ttl);
}

/**
 * 'yes': send it to Nexdrop. 'no': leave it to the browser. 'maybe': the browser
 * hasn't said enough yet (when a download is announced it often has no file
 * name, no size and a placeholder type), so decide again as details arrive.
 */
function classify(item, s) {
  const url = item.finalUrl || item.url || '';
  if (!/^https?:/i.test(url)) return 'no'; // blob:, data:, file:, ftp:...
  if (url.startsWith(NEXDROP_SERVER)) return 'no';
  if (item.state && item.state !== 'in_progress') return 'no'; // finished or failed already
  if (skipOnce.has(item.url) || skipOnce.has(url)) return 'no';
  if (isIgnoredSite(url, s.ignoreSites)) return 'no';

  const types = new Set(words(s.fileTypes));
  const ext = extensionOf(item.filename) || extensionOf(url);
  if (ext && types.has(ext)) return 'yes';

  const size = item.fileSize > 0 ? item.fileSize : item.totalBytes > 0 ? item.totalBytes : 0;
  if (size >= s.minSizeMB * 1024 * 1024) return 'yes';

  const mime = (item.mime || '').toLowerCase();
  // Plainly a web page or small data: not a file worth taking over.
  if (/^(text\/(html|css|csv|xml)|image\/|application\/(json|xml|javascript|xhtml))/.test(mime)) return 'no';
  // Any other known binary type is a file.
  if (mime && mime !== 'text/plain') return 'yes';
  return 'maybe';
}

const undecided = new Set(); // download ids waiting for more details

async function handleDownload(item) {
  try {
    const s = await getSettings();
    if (!s.autoIntercept) return;
    const verdict = classify(item, s);
    if (verdict === 'maybe') {
      undecided.add(item.id);
      return;
    }
    undecided.delete(item.id);
    if (verdict !== 'yes') return;

    const url = item.finalUrl || item.url;
    if (recentlySent.has(url)) return;
    remember(recentlySent, url, 4000);

    const filename = item.filename ? item.filename.split(/[\/]/).pop() : undefined;
    const ok = await sendToNexdrop(url, item.referrer, filename);
    if (ok) {
      // Nexdrop has it: drop the browser's own copy.
      chrome.downloads.cancel(item.id, () => {
        chrome.runtime.lastError;
        chrome.downloads.erase({ id: item.id });
      });
    }
  } catch (e) {
    console.warn('Nexdrop could not take over a download:', e);
  }
}

chrome.downloads.onCreated.addListener(handleDownload);

chrome.downloads.onChanged.addListener(async (delta) => {
  if (!undecided.has(delta.id)) return;
  if (!(delta.filename || delta.mime || delta.totalBytes || delta.fileSize || delta.state)) return;
  const [item] = await chrome.downloads.search({ id: delta.id });
  if (!item) return undecided.delete(delta.id);
  if (item.state !== 'in_progress') undecided.delete(delta.id);
  else handleDownload(item);
});

// ── videos and media files seen in each tab ──────────────────────────────────
//
// Players request their playlist or media file; those requests are noted per
// tab. Kept in session storage because the worker is stopped and restarted by
// the browser. Writes are serialised so a burst of requests can't overwrite
// each other.

const STREAM_RE = /\.(m3u8|mpd)$/i;
const MEDIA_EXT_RE = /\.(mp4|m4v|webm|mkv|mov|avi|flv|mp3|m4a|aac|flac|ogg|opus|wav)$/i;
const MAX_MEDIA = 8;
const MIN_MEDIA_BYTES = 300 * 1024;
const mediaKey = (tabId) => `media:${tabId}`;

async function getMedia(tabId) {
  if (tabId == null || tabId < 0) return [];
  const key = mediaKey(tabId);
  const stored = await chrome.storage.session.get(key);
  return stored[key] || [];
}

let mediaWrites = Promise.resolve();
function noteMedia(tabId, entry) {
  mediaWrites = mediaWrites.then(() => recordMedia(tabId, entry)).catch(() => {});
  return mediaWrites;
}

async function recordMedia(tabId, entry) {
  if (tabId < 0) return;
  const list = await getMedia(tabId);
  // The same video is asked for again with a fresh token or another byte range: keep one entry.
  const identity = (u) => {
    try {
      const x = new URL(u);
      return x.origin + x.pathname;
    } catch (_) {
      return u;
    }
  };
  const id = identity(entry.url);
  const existing = list.findIndex((m) => identity(m.url) === id);
  let page = '';
  let title = '';
  try {
    const tab = await chrome.tabs.get(tabId);
    page = tab.url || '';
    title = tab.title || '';
  } catch (_) { /* tab already gone */ }
  const next = { ...entry, page, title, at: Date.now() };
  if (existing >= 0) {
    next.size = Math.max(next.size || 0, list[existing].size || 0);
    list.splice(existing, 1);
  }
  list.unshift(next);
  const kept = list.slice(0, MAX_MEDIA);
  await chrome.storage.session.set({ [mediaKey(tabId)]: kept });
  chrome.action.setBadgeBackgroundColor({ tabId, color: '#5b8cff' });
  chrome.action.setBadgeText({ tabId, text: String(kept.length) });
}

const isBridge = (u) => u.port === '18888' && (u.hostname === '127.0.0.1' || u.hostname === 'localhost');

chrome.webRequest.onBeforeRequest.addListener(
  (details) => {
    try {
      const u = new URL(details.url);
      if (isBridge(u)) return;
      if (STREAM_RE.test(u.pathname)) noteMedia(details.tabId, { kind: 'stream', url: details.url });
    } catch (_) { /* not a URL */ }
  },
  { urls: ['<all_urls>'] },
);

chrome.webRequest.onHeadersReceived.addListener(
  (details) => {
    try {
      if (details.statusCode !== 200 && details.statusCode !== 206) return;
      const u = new URL(details.url);
      if (isBridge(u) || STREAM_RE.test(u.pathname)) return;
      const header = (name) => (details.responseHeaders || []).find((h) => h.name.toLowerCase() === name)?.value || '';
      const mime = header('content-type').split(';')[0].trim().toLowerCase();
      // Segments of a stream are not videos of their own.
      if (/mp2t|mp4-segment|mpegurl|dash/.test(mime)) return;
      const mediaType = /^(video|audio)\//.test(mime) || (mime === 'application/octet-stream' && MEDIA_EXT_RE.test(u.pathname));
      if (!mediaType) return;
      const range = header('content-range').match(/\/(\d+)\s*$/);
      const size = range ? Number(range[1]) : Number(header('content-length')) || 0;
      if (size < MIN_MEDIA_BYTES) return;
      noteMedia(details.tabId, { kind: 'file', url: details.url, mime, size });
    } catch (_) { /* ignore */ }
  },
  { urls: ['<all_urls>'], types: ['media', 'xmlhttprequest', 'other'] },
  ['responseHeaders'],
);

// A new page in the tab means new videos.
chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (changeInfo.status === 'loading' && changeInfo.url) {
    chrome.storage.session.remove(mediaKey(tabId));
    chrome.action.setBadgeText({ tabId, text: '' });
  }
});
chrome.tabs.onRemoved.addListener((tabId) => chrome.storage.session.remove(mediaKey(tabId)));

/** A readable name for a video from the page it plays on. */
function nameFromTitle(title) {
  const clean = String(title || '')
    .replace(/[\\/:*?"<>|\u0000-\u001f]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, 120);
  return clean || undefined;
}

/** The entry most likely to be the whole video: a master playlist, else the biggest file. */
function bestMedia(list) {
  const stream = list.find((m) => m.kind === 'stream' && /master|playlist|manifest|\.mpd/i.test(new URL(m.url).pathname));
  if (stream) return stream;
  const files = list.filter((m) => m.kind === 'file').sort((a, b) => (b.size || 0) - (a.size || 0));
  return files[0] || list.find((m) => m.kind === 'stream') || list[0];
}

// ── menus ────────────────────────────────────────────────────────────────────

chrome.runtime.onInstalled.addListener(async () => {
  chrome.contextMenus.create({
    id: 'nexdrop-download-item',
    title: 'Download with Nexdrop',
    contexts: ['link', 'image', 'video', 'audio'],
  });
  chrome.contextMenus.create({
    id: 'nexdrop-grab-all-links',
    title: 'Grab all links on this page with Nexdrop',
    contexts: ['page'],
  });
  const stored = await chrome.storage.local.get('settings');
  if (!stored.settings) await chrome.storage.local.set({ settings: DEFAULTS });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId === 'nexdrop-download-item') {
    const target = info.linkUrl || info.srcUrl;
    if (target) sendToNexdrop(target, tab && tab.url);
  } else if (info.menuItemId === 'nexdrop-grab-all-links' && tab && tab.url) {
    try {
      await fetch(`${NEXDROP_SERVER}/parse-page`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url: tab.url }),
      });
    } catch (_) { /* the app isn't running */ }
  }
});

// ── messages from the page script, popup and options ─────────────────────────

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  const reply = (promise) => {
    promise.then(sendResponse).catch((err) => sendResponse({ success: false, error: String(err) }));
    return true; // keep the channel open for the async answer
  };

  switch (message.type) {
    case 'SEND_TO_NEXDROP':
      return reply(sendToNexdrop(message.url, message.referer || (sender.tab && sender.tab.url), message.filename).then((ok) => ({ success: ok })));

    case 'SEND_MEDIA':
      return reply(
        (async () => {
          const tabId = message.tabId ?? (sender.tab && sender.tab.id);
          const list = await getMedia(tabId);
          const entry = list.find((m) => m.url === message.url);
          const ok = await sendToNexdrop(message.url, (entry && entry.page) || message.page || (sender.tab && sender.tab.url), nameFromTitle(entry && entry.title));
          return { success: ok };
        })(),
      );

    case 'GET_MEDIA':
      return reply(
        getMedia(message.tabId ?? (sender.tab && sender.tab.id)).then((media) => ({ media, best: media.length ? bestMedia(media) : null })),
      );

    case 'SKIP_NEXT': // Alt+click: let the browser handle this one itself
      remember(skipOnce, message.url, 8000);
      sendResponse({ ok: true });
      return false;

    case 'GET_SETTINGS':
      return reply(getSettings());

    case 'CHECK_CONNECTION':
      return reply(
        fetch(`${NEXDROP_SERVER}/status`)
          .then((r) => r.json())
          .then((data) => ({ connected: true, data }))
          .catch(() => ({ connected: false })),
      );

    default:
      return false;
  }
});
