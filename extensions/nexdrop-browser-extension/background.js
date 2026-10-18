// Nexdrop Browser Integration — Service Worker (Manifest V3)
const NEXDROP_SERVER = 'http://127.0.0.1:18888';

// ── Streaming videos (HLS .m3u8 / DASH .mpd) seen in each tab ─────────────────
// A page's player requests its playlist; we note those requests so the video
// button and the popup can offer the stream to Nexdrop. Kept in session
// storage because the service worker is stopped and restarted by the browser.
const STREAM_RE = /\.(m3u8|mpd)$/i;
const MAX_STREAMS = 6;
const streamKey = (tabId) => `streams:${tabId}`;

async function getStreams(tabId) {
  if (tabId == null || tabId < 0) return [];
  const key = streamKey(tabId);
  const stored = await chrome.storage.session.get(key);
  return stored[key] || [];
}

// Requests arrive in bursts (master playlist, then each quality): handle them one at a time
// so two writes never overwrite each other.
let streamWrites = Promise.resolve();
function noteStream(tabId, url) {
  streamWrites = streamWrites.then(() => recordStream(tabId, url)).catch(() => {});
  return streamWrites;
}

async function recordStream(tabId, url) {
  if (tabId < 0) return;
  const list = await getStreams(tabId);
  if (list.some((s) => s.url === url)) return;
  let page = '';
  try {
    page = (await chrome.tabs.get(tabId)).url || '';
  } catch (_) { /* tab already gone */ }
  list.unshift({ url, page, at: Date.now() });
  const kept = list.slice(0, MAX_STREAMS);
  await chrome.storage.session.set({ [streamKey(tabId)]: kept });
  chrome.action.setBadgeBackgroundColor({ tabId, color: '#5b8cff' });
  chrome.action.setBadgeText({ tabId, text: String(kept.length) });
}

chrome.webRequest.onBeforeRequest.addListener(
  (details) => {
    try {
      const u = new URL(details.url);
      if (u.port === '18888' && u.hostname === '127.0.0.1') return;
      if (STREAM_RE.test(u.pathname)) noteStream(details.tabId, details.url);
    } catch (_) { /* not a URL */ }
  },
  { urls: ['<all_urls>'] },
);

// A new page in the tab means new videos.
chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (changeInfo.status === 'loading' && changeInfo.url) {
    chrome.storage.session.remove(streamKey(tabId));
    chrome.action.setBadgeText({ tabId, text: '' });
  }
});
chrome.tabs.onRemoved.addListener((tabId) => chrome.storage.session.remove(streamKey(tabId)));

/** The stream most likely to be the whole video: a master playlist / manifest rather than one quality of it. */
function bestStream(list) {
  const looksLikeMaster = (s) => /master|playlist|manifest|\.mpd/i.test(new URL(s.url).pathname);
  return list.find(looksLikeMaster) || list[0];
}

// Setup Context Menus
chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: 'nexdrop-download-item',
    title: 'Download with Nexdrop',
    contexts: ['link', 'image', 'video', 'audio'],
  });

  chrome.contextMenus.create({
    id: 'nexdrop-grab-all-links',
    title: 'Grab All Links on Page with Nexdrop',
    contexts: ['page'],
  });

  chrome.storage.local.set({
    autoIntercept: true,
    showVideoGrabber: true,
  });
});

// Handle Context Menu clicks
chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId === 'nexdrop-download-item') {
    const targetUrl = info.linkUrl || info.srcUrl;
    if (targetUrl) {
      sendToNexdrop(targetUrl, tab?.url);
    }
  } else if (info.menuItemId === 'nexdrop-grab-all-links') {
    if (tab?.url) {
      // Trigger page parse
      try {
        await fetch(`${NEXDROP_SERVER}/parse-page`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ url: tab.url }),
        });
        showNotification('Page links sent to Nexdrop Grabber');
      } catch (err) {
        showNotification('Failed to communicate with Nexdrop Desktop');
      }
    }
  }
});

// Intercept browser downloads
chrome.downloads.onCreated.addListener(async (downloadItem) => {
  const { autoIntercept } = await chrome.storage.local.get(['autoIntercept']);
  if (autoIntercept === false) return;

  const url = downloadItem.url || downloadItem.finalUrl;
  if (!url || url.startsWith('blob:') || url.startsWith('data:')) return;

  // Check if extension is downloadable
  const filename = downloadItem.filename || '';
  const isTargetExtension = /\.(zip|rar|7z|tar|gz|iso|dmg|exe|msi|apk|mp4|mkv|webm|avi|mp3|flac|pdf|docx|xlsx|torrent)$/i.test(
    url.split('?')[0]
  );

  if (isTargetExtension || downloadItem.fileSize > 5 * 1024 * 1024) {
    try {
      const ok = await sendToNexdrop(url, downloadItem.referrer, filename);
      if (ok) {
        chrome.downloads.cancel(downloadItem.id, () => {
          chrome.downloads.erase({ id: downloadItem.id });
        });
        showNotification(`Downloading with Nexdrop: ${filename || 'file'}`);
      }
    } catch (e) {
      console.warn('Nexdrop intercept skipped:', e);
    }
  }
});

// Message handler for content script and popup
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === 'SEND_TO_NEXDROP') {
    sendToNexdrop(message.url, sender.tab?.url, message.filename)
      .then((ok) => sendResponse({ success: ok }))
      .catch((err) => sendResponse({ success: false, error: err.toString() }));
    return true; // Keep channel open for async response
  } else if (message.type === 'GET_STREAMS') {
    getStreams(message.tabId ?? sender.tab?.id).then((streams) => sendResponse({ streams, best: streams.length ? bestStream(streams) : null }));
    return true;
  } else if (message.type === 'SEND_STREAM') {
    // Nexdrop opens its window so the quality can be chosen there.
    sendToNexdrop(message.url, message.page || sender.tab?.url)
      .then((ok) => sendResponse({ success: ok }))
      .catch((err) => sendResponse({ success: false, error: err.toString() }));
    return true;
  } else if (message.type === 'CHECK_CONNECTION') {
    fetch(`${NEXDROP_SERVER}/status`)
      .then((res) => res.json())
      .then((data) => sendResponse({ connected: true, data }))
      .catch(() => sendResponse({ connected: false }));
    return true;
  }
});

async function sendToNexdrop(url, referer, filename) {
  try {
    const res = await fetch(`${NEXDROP_SERVER}/download`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        url,
        referer: referer || undefined,
        filename: filename || undefined,
      }),
    });
    return res.ok;
  } catch (err) {
    console.error('Failed to send to Nexdrop:', err);
    return false;
  }
}

function showNotification(message) {
  console.log('[Nexdrop]', message);
}
