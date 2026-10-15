document.addEventListener('DOMContentLoaded', async () => {
  const statusBadge = document.getElementById('statusBadge');
  const statusText = document.getElementById('statusText');
  const urlInput = document.getElementById('urlInput');
  const downloadBtn = document.getElementById('downloadBtn');
  const interceptToggle = document.getElementById('interceptToggle');

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
