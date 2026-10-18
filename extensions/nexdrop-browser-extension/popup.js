document.addEventListener('DOMContentLoaded', async () => {
  const statusBadge = document.getElementById('statusBadge');
  const statusText = document.getElementById('statusText');
  const urlInput = document.getElementById('urlInput');
  const downloadBtn = document.getElementById('downloadBtn');
  const interceptToggle = document.getElementById('interceptToggle');

  const sizeText = (n) => (n > 0 ? (n >= 1073741824 ? (n / 1073741824).toFixed(1) + ' GB' : Math.max(1, Math.round(n / 1048576)) + ' MB') : '');

  // Videos and media files this tab has loaded (HLS / DASH playlists, video and audio files)
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  chrome.runtime.sendMessage({ type: 'GET_MEDIA', tabId: tab && tab.id }, (reply) => {
    if (chrome.runtime.lastError || !reply || !reply.media || reply.media.length === 0) return;
    const box = document.getElementById('videos');
    const list = document.getElementById('videoList');
    box.hidden = false;
    reply.media.forEach((m) => {
      const u = new URL(m.url);
      const row = document.createElement('div');
      row.className = 'video-row';

      const tag = document.createElement('span');
      tag.className = 'kind';
      tag.textContent = m.kind === 'stream' ? (/\.mpd$/i.test(u.pathname) ? 'DASH' : 'HLS') : ((m.mime || 'file').split('/').pop() || 'file').toUpperCase().slice(0, 5);

      const name = document.createElement('div');
      name.className = 'name';
      name.title = m.url;
      name.textContent = decodeURIComponent(u.pathname.split('/').filter(Boolean).slice(-2).join('/')) || u.hostname;

      const size = document.createElement('span');
      size.className = 'size';
      size.textContent = sizeText(m.size);

      const btn = document.createElement('button');
      btn.className = 'btn-small';
      btn.textContent = 'Download';
      btn.addEventListener('click', () => {
        chrome.runtime.sendMessage({ type: 'SEND_MEDIA', url: m.url, tabId: tab && tab.id, page: m.page }, (res) => {
          btn.textContent = res && res.success ? 'Sent' : 'Not running';
          setTimeout(() => { btn.textContent = 'Download'; }, 1600);
        });
      });
      row.append(tag, name, size, btn);
      list.appendChild(row);
    });
  });

  // Connection to the desktop app
  chrome.runtime.sendMessage({ type: 'CHECK_CONNECTION' }, (response) => {
    if (response && response.connected) {
      statusBadge.className = 'status-badge status-online';
      statusText.textContent = 'Nexdrop is running';
    } else {
      statusBadge.className = 'status-badge status-offline';
      statusText.textContent = 'Nexdrop isn’t running';
    }
  });

  // Capture switch (kept in the settings object the options page edits)
  const { settings } = await chrome.storage.local.get('settings');
  interceptToggle.checked = settings ? settings.autoIntercept !== false : true;
  interceptToggle.addEventListener('change', async () => {
    const stored = (await chrome.storage.local.get('settings')).settings || {};
    await chrome.storage.local.set({ settings: { ...stored, autoIntercept: interceptToggle.checked } });
  });

  document.getElementById('optionsLink').addEventListener('click', (e) => {
    e.preventDefault();
    chrome.runtime.openOptionsPage();
    window.close();
  });

  downloadBtn.addEventListener('click', () => {
    const url = urlInput.value.trim();
    if (!url) return;
    downloadBtn.textContent = 'Sending…';
    chrome.runtime.sendMessage({ type: 'SEND_TO_NEXDROP', url, referer: tab && tab.url }, (res) => {
      if (res && res.success) {
        downloadBtn.textContent = 'Sent to Nexdrop';
        urlInput.value = '';
      } else {
        downloadBtn.textContent = 'Nexdrop isn’t running';
      }
      setTimeout(() => { downloadBtn.textContent = 'Download with Nexdrop'; }, 1800);
    });
  });
});
