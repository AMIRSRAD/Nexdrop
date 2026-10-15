// Nexdrop Browser Integration — Service Worker (Manifest V3)
const NEXDROP_SERVER = 'http://127.0.0.1:18888';

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
  const isTargetExtension = /\.(zip|rar|7z|tar|gz|iso|dmg|exe|msi|apk|mp4|mkv|webm|avi|mp3|flac|pdf|docx|xlsx)$/i.test(
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
