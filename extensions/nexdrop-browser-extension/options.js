const DEFAULTS = {
  autoIntercept: true,
  interceptLinks: true,
  videoButton: true,
  fileTypes:
    'zip rar 7z tar gz tgz xz bz2 zst iso img dmg pkg exe msi msix deb rpm apk appimage bin torrent ' +
    'mp4 mkv webm avi mov flv wmv m4v mpg mpeg mp3 flac wav aac ogg opus m4a ' +
    'pdf epub doc docx xls xlsx ppt pptx',
  ignoreSites: '',
  minSizeMB: 10,
};

const fields = ['autoIntercept', 'interceptLinks', 'videoButton', 'fileTypes', 'ignoreSites', 'minSizeMB'];
const el = (id) => document.getElementById(id);
let timer;

function show(settings) {
  for (const key of fields) {
    const input = el(key);
    if (input.type === 'checkbox') input.checked = !!settings[key];
    else input.value = settings[key];
  }
}

function read() {
  const out = {};
  for (const key of fields) {
    const input = el(key);
    out[key] = input.type === 'checkbox' ? input.checked : input.type === 'number' ? Math.max(1, Number(input.value) || DEFAULTS.minSizeMB) : input.value;
  }
  return out;
}

async function save() {
  await chrome.storage.local.set({ settings: read() });
  el('saved').classList.add('on');
  setTimeout(() => el('saved').classList.remove('on'), 1200);
}

(async () => {
  const { settings } = await chrome.storage.local.get('settings');
  show({ ...DEFAULTS, ...(settings || {}) });
  for (const key of fields) {
    el(key).addEventListener('input', () => {
      clearTimeout(timer);
      timer = setTimeout(save, 350);
    });
  }
  el('reset').addEventListener('click', async () => {
    show(DEFAULTS);
    await save();
  });
})();
