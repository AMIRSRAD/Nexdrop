document.addEventListener('DOMContentLoaded', async () => {
  const statusBadge = document.getElementById('statusBadge');
  const statusText = document.getElementById('statusText');
  const urlInput = document.getElementById('urlInput');
  const downloadBtn = document.getElementById('downloadBtn');
  const interceptToggle = document.getElementById('interceptToggle');

  // Streaming videos the page loaded (HLS / DASH)
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  chrome.runtime.sendMessage({ type: 'GET_STREAMS', tabId: tab?.id }, (reply) => {
    if (chrome.runtime.lastError || !reply || !reply.streams || reply.streams.length === 0) return;
    const box = document.getElementById('videos');
    const list = document.getElementById('videoList');
    box.hidden = false;
    reply.streams.forEach((s) => {
      const u = new URL(s.url);
      const row = document.createElement('div');
      row.className = 'video-row';
      const name = document.createElement('div');
      name.className = 'name';
      name.title = s.url;
      name.textContent = u.pathname.split('/').filter(Boolean).slice(-2).join('/') || u.hostname;
      const kind = document.createElement('span');
      kind.className = 'kind';
      kind.textContent = /\.mpd$/i.test(u.pathname) ? 'DASH' : 'HLS';
      name.appendChild(kind);
      const btn = document.createElement('button');
      btn.className = 'btn-small';
      btn.textContent = 'Download';
      btn.addEventListener('click', () => {
        chrome.runtime.sendMessage({ type: 'SEND_STREAM', url: s.url, page: s.page }, (res) => {
          btn.textContent = res && res.success ? 'Sent' : 'Failed';
          setTimeout(() => { btn.textContent = 'Download'; }, 1500);
        });
      });
      row.append(name, btn);
      list.appendChild(row);
    });
  });

  // Check connection status
  chrome.runtime.sendMessage({ type: 'CHECK_CONNECTION' }, (response) => {
    if (response && response.connected) {
      statusBadge.className = 'status-badge status-online';
      statusText.textContent = 'Desktop Online (:18888)';
    } else {
      statusBadge.className = 'status-badge status-offline';
      statusText.textContent = 'App Offline';
    }
  });

  // Load saved settings
  const { autoIntercept } = await chrome.storage.local.get(['autoIntercept']);
  if (autoIntercept !== undefined) {
    interceptToggle.checked = autoIntercept;
  }

  interceptToggle.addEventListener('change', () => {
    chrome.storage.local.set({ autoIntercept: interceptToggle.checked });
  });

  downloadBtn.addEventListener('click', async () => {
    const url = urlInput.value.trim();
    if (!url) return;

    downloadBtn.textContent = 'Sending...';
    chrome.runtime.sendMessage({ type: 'SEND_TO_NEXDROP', url }, (res) => {
      if (res && res.success) {
        downloadBtn.textContent = 'Download Started!';
        urlInput.value = '';
        setTimeout(() => { downloadBtn.textContent = 'Download with Nexdrop'; }, 1500);
      } else {
        downloadBtn.textContent = 'Failed to Send';
        setTimeout(() => { downloadBtn.textContent = 'Download with Nexdrop'; }, 2000);
      }
    });
  });
});
