// Nexdrop Video & Audio Stream Sniffer Content Script
(function () {
  const processedMedia = new WeakSet();

  function scanMediaElements() {
    const mediaElements = document.querySelectorAll('video, audio');

    mediaElements.forEach((el) => {
      if (processedMedia.has(el)) return;
      processedMedia.add(el);

      attachNexdropButton(el);
    });
  }

  function attachNexdropButton(mediaEl) {
    const container = mediaEl.parentElement;
    if (!container) return;

    // Create floating grabber button
    const btn = document.createElement('div');
    btn.className = 'nexdrop-video-grabber-btn';
    btn.innerHTML = `
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
        <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"></polygon>
      </svg>
      <span>Download with Nexdrop</span>
    `;

    Object.assign(btn.style, {
      position: 'absolute',
      zIndex: '2147483647',
      display: 'none',
      alignItems: 'center',
      gap: '6px',
      padding: '6px 12px',
      backgroundColor: '#10121a',
      color: '#ffffff',
      border: '1px solid rgba(255, 255, 255, 0.15)',
      borderRadius: '8px',
      fontSize: '11px',
      fontWeight: '600',
      fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',
      boxShadow: '0 8px 24px rgba(0,0,0,0.4)',
      cursor: 'pointer',
      transition: 'all 0.2s cubic-bezier(0.16, 1, 0.3, 1)',
      userSelect: 'none',
      backdropFilter: 'blur(12px)',
    });

    btn.addEventListener('mouseenter', () => {
      btn.style.backgroundColor = '#4f46e5';
      btn.style.transform = 'translateY(-1px) scale(1.02)';
    });

    btn.addEventListener('mouseleave', () => {
      btn.style.backgroundColor = '#10121a';
      btn.style.transform = 'translateY(0) scale(1)';
    });

    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      e.preventDefault();

      let src = mediaEl.currentSrc || mediaEl.src;
      if (!src) {
        const source = mediaEl.querySelector('source');
        src = source ? source.src : null;
      }

      if (src && (src.startsWith('http://') || src.startsWith('https://'))) {
        chrome.runtime.sendMessage({
          type: 'SEND_TO_NEXDROP',
          url: src,
          filename: document.title ? `${document.title.replace(/[\\/:*?"<>|]/g, '_')}.mp4` : undefined,
        });

        btn.innerHTML = `<span>Sent to Nexdrop!</span>`;
        setTimeout(() => {
          btn.innerHTML = `
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
              <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"></polygon>
            </svg>
            <span>Download with Nexdrop</span>
          `;
        }, 2000);
      }
    });

    // Positioning
    function positionButton() {
      const rect = mediaEl.getBoundingClientRect();
      if (rect.width > 120 && rect.height > 80) {
        btn.style.top = `${window.scrollY + rect.top + 10}px`;
        btn.style.right = `${window.innerWidth - (window.scrollX + rect.right) + 10}px`;
        btn.style.display = 'flex';
      }
    }

    mediaEl.addEventListener('mouseenter', positionButton);
    mediaEl.addEventListener('play', positionButton);
    btn.addEventListener('mouseenter', () => { btn.style.display = 'flex'; });

    let hideTimeout;
    mediaEl.addEventListener('mouseleave', () => {
      hideTimeout = setTimeout(() => {
        btn.style.display = 'none';
      }, 1500);
    });
    btn.addEventListener('mouseleave', () => {
      btn.style.display = 'none';
    });

    document.body.appendChild(btn);
  }

  // Scan initially and observe DOM additions
  scanMediaElements();
  const observer = new MutationObserver(() => scanMediaElements());
  observer.observe(document.body, { childList: true, subtree: true });
})();
