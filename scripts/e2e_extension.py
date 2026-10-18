"""Tests the browser extension in REAL browsers (Edge/Chrome and Firefox).

    pip install selenium
    python scripts/e2e_extension.py            # every browser found
    python scripts/e2e_extension.py firefox    # just one: edge | chrome | firefox

A fake Nexdrop bridge listens on 127.0.0.1:18888 and records what the extension
sends; a small test website offers a download link, a PDF, a video and an HLS
stream. Close Nexdrop first (it uses the same port).

Checks: links and downloads are handed over with the page's cookies, referer
and user agent; Alt-click is left to the browser; videos/PDFs you click are
caught; the video button finds the playlist or file; ignored sites stay with the
browser; nothing is sent when the app is not running.
"""
import http.server, json, os, shutil, socketserver, sys, tempfile, threading, time

from selenium import webdriver
from selenium.webdriver.common.action_chains import ActionChains
from selenium.webdriver.common.by import By
from selenium.webdriver.common.keys import Keys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXT = os.path.join(ROOT, 'extensions', 'nexdrop-browser-extension')
results = []


def check(name, ok, detail=''):
    results.append((name, ok))
    print(('PASS  ' if ok else 'FAIL  ') + name + (f'  [{detail}]' if detail and not ok else ''), flush=True)


# ───────────────────────────── fake Nexdrop bridge ─────────────────────────────
POSTS = []
BRIDGE_UP = True


class Bridge(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *a):
        pass

    def _cors(self):
        origin = self.headers.get('Origin')
        if origin:
            self.send_header('Access-Control-Allow-Origin', origin)
            self.send_header('Access-Control-Allow-Headers', 'Content-Type')
            self.send_header('Access-Control-Allow-Methods', 'GET, POST, OPTIONS')

    def do_OPTIONS(self):
        self.send_response(204); self._cors(); self.send_header('Content-Length', '0'); self.end_headers()

    def do_GET(self):
        body = json.dumps({'name': 'Nexdrop', 'status': 'online'}).encode()
        self.send_response(200); self._cors(); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(body))); self.end_headers(); self.wfile.write(body)

    def do_POST(self):
        n = int(self.headers.get('Content-Length', 0))
        data = json.loads(self.rfile.read(n) or b'{}')
        POSTS.append({'path': self.path, 'origin': self.headers.get('Origin'), 't': round(time.time() % 1000, 1), **data})
        body = json.dumps({'success': True, 'prompt': True}).encode()
        self.send_response(200); self._cors(); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(body))); self.end_headers(); self.wfile.write(body)


class Threaded(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def handle_error(self, request, client_address):
        pass  # browsers drop connections all the time


# ───────────────────────────── test website ─────────────────────────────
VIDEO = bytes((i * 7) % 251 for i in range(2 * 1024 * 1024))
PDF = b'%PDF-1.4\n' + b'x' * 20000
ZIP = b'PK\x03\x04' + bytes(range(256)) * 400


class Site(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *a):
        pass

    def send(self, code, ctype, body, extra=None):
        self.send_response(code)
        self.send_header('Content-Type', ctype)
        self.send_header('Content-Length', str(len(body)))
        for k, v in (extra or {}).items():
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        path = self.path.split('?')[0]
        if path == '/':
            page = b"""<!doctype html><title>Test Shop</title><body>
              <a id=zip href="/dl/archive?id=7">Download the archive</a><br>
              <a id=pdf href="/files/report.pdf">Read the report</a><br>
              <a id=mp4 href="/files/clip.mp4">Watch the clip</a><br>
              <a id=page href="/other.html">An ordinary page</a>"""
            self.send(200, 'text/html', page, {'Set-Cookie': 'sid=abc123; Path=/'})
        elif path == '/other.html':
            self.send(200, 'text/html', b'<!doctype html><title>Other</title><p>hello')
        elif path == '/links.html':
            page = b"""<!doctype html><title>Link Page</title><body>
              <a href="#top">top</a> <a href="/other.html">ordinary page</a> <a href="mailto:a@b.c">mail</a>
              <div id=picked>Pick these: <a href="/files/song-one.mp3">song one</a> and <a href="/files/song-two.mp3">song two</a>
                 plus plain text https://example.com/files/doc.pdf here</div>
              <p>Not these: <a href="/files/outside.zip">outside</a></p>"""
            self.send(200, 'text/html', page)
        elif path == '/video.html':
            page = b"""<!doctype html><title>My Holiday Video</title><body style="margin:0">
              <video id=v src="/files/clip.mp4" width=640 height=360 muted loop style="display:block"></video>
              <script>fetch('/stream/master.m3u8?token=1').catch(()=>{});</script>"""
            self.send(200, 'text/html', page)
        elif path == '/plainvideo.html':
            page = b"""<!doctype html><title>Plain Clip Page</title><body style="margin:0">
              <video id=v src="/files/clip.mp4" width=640 height=360 muted loop style="display:block"></video><div id=away style="height:300px">nothing here</div>"""
            self.send(200, 'text/html', page)
        elif path == '/dl/movie':
            # A slow attachment, like a real download: still arriving when the browser announces it.
            self.send_response(200)
            self.send_header('Content-Type', 'video/mp4')
            self.send_header('Content-Disposition', 'attachment; filename="movie.mp4"')
            self.send_header('Content-Length', str(len(VIDEO)))
            self.end_headers()
            try:
                for i in range(0, len(VIDEO), 65536):
                    self.wfile.write(VIDEO[i:i + 65536])
                    self.wfile.flush()
                    time.sleep(0.15)
            except Exception:
                pass
        elif path == '/dl/archive':
            # Served slowly, like a real download, so the browser is still receiving when it announces it.
            self.send_response(200)
            self.send_header('Content-Type', 'application/octet-stream')
            self.send_header('Content-Disposition', 'attachment; filename="archive.dat"')
            self.send_header('Content-Length', str(len(ZIP)))
            self.end_headers()
            sent = 0
            try:
                for i in range(0, len(ZIP), 8192):
                    self.wfile.write(ZIP[i:i + 8192])
                    self.wfile.flush()
                    sent += 8192
                    time.sleep(0.2)
            except Exception:  # the browser hung up (it was cancelled)
                pass
        elif path == '/files/report.pdf':
            self.send(200, 'application/pdf', PDF)
        elif path == '/files/clip.mp4':
            rng = self.headers.get('Range')
            if rng and rng.startswith('bytes='):
                a, _, b = rng[6:].partition('-')
                start, end = int(a or 0), min(int(b) if b else len(VIDEO) - 1, len(VIDEO) - 1)
                self.send(206, 'video/mp4', VIDEO[start:end + 1], {'Content-Range': f'bytes {start}-{end}/{len(VIDEO)}', 'Accept-Ranges': 'bytes'})
            else:
                self.send(200, 'video/mp4', VIDEO, {'Accept-Ranges': 'bytes'})
        elif path == '/stream/master.m3u8':
            self.send(200, 'application/vnd.apple.mpegurl', b'#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000\nlow.m3u8\n')
        else:
            self.send(404, 'text/plain', b'nope')

    do_HEAD = do_GET


# ───────────────────────────── browsers ─────────────────────────────

def start_browser(kind, download_dir):
    if kind in ('edge', 'chrome'):
        if kind == 'edge':
            from selenium.webdriver.edge.options import Options
            opts = Options()
            driver_cls = webdriver.Edge
        else:
            from selenium.webdriver.chrome.options import Options
            opts = Options()
            driver_cls = webdriver.Chrome
        if os.environ.get('HEADLESS'):
            opts.add_argument('--headless=new')
        opts.add_argument(f'--load-extension={EXT}')
        opts.add_argument('--disable-features=DisableLoadExtensionCommandLineSwitch')
        opts.add_argument('--window-size=1100,800')
        opts.add_experimental_option('prefs', {'download.default_directory': download_dir, 'download.prompt_for_download': False, 'plugins.always_open_pdf_externally': False})
        opts.add_experimental_option('excludeSwitches', ['disable-extensions'])
        driver = driver_cls(options=opts)
        return driver
    from selenium.webdriver.firefox.options import Options
    opts = Options()
    opts.add_argument('-headless')
    opts.set_preference('browser.download.folderList', 2)
    opts.set_preference('browser.download.dir', download_dir)
    opts.set_preference('browser.download.useDownloadDir', True)
    opts.set_preference('browser.helperApps.neverAsk.saveToDisk', 'application/zip,application/octet-stream')
    opts.set_preference('pdfjs.disabled', False)
    from selenium.webdriver.firefox.service import Service
    driver = webdriver.Firefox(options=opts, service=Service(service_args=['--allow-system-access']))
    driver.install_addon(EXT, temporary=True)
    return driver



def extension_origin(driver, kind):
    """The extension's own address (needed to open its pages)."""
    if kind == 'firefox':
        driver.set_context('chrome')
        uuids = driver.execute_script("return Services.prefs.getStringPref('extensions.webextensions.uuids')")
        driver.set_context('content')
        return 'moz-extension://' + json.loads(uuids)['nexdrop@nexdrop.app']
    for t in driver.execute_cdp_cmd('Target.getTargets', {})['targetInfos']:
        if t['url'].startswith('chrome-extension://') and t['url'].endswith('background.js'):
            return t['url'].rsplit('/', 1)[0]
    raise RuntimeError('extension not found')


def browser_download(driver, kind, url, filename=None):
    """Starts a download the way a link would: through the browser's download manager."""
    page = driver.current_window_handle
    driver.switch_to.new_window('tab')
    driver.get(extension_origin(driver, kind) + '/options.html')
    driver.execute_async_script("const cb = arguments[arguments.length - 1]; chrome.downloads.download(Object.assign({ url: arguments[0] }, arguments[1] ? { filename: arguments[1] } : {}), () => cb(true));", url, filename)
    driver.close()
    driver.switch_to.window(page)


def extension_call(driver, kind, script, *args):
    """Runs JS in an extension page (to talk to the background script like the menus do)."""
    page = driver.current_window_handle
    driver.switch_to.new_window('tab')
    driver.get(extension_origin(driver, kind) + '/options.html')
    try:
        return driver.execute_async_script(script, *args)
    finally:
        driver.close()
        driver.switch_to.window(page)


def many_posts(n):
    return [p for p in POSTS[n:] if p.get('path') == '/download-many']


def wait_for(cond, timeout=12, step=0.25):
    end = time.time() + timeout
    while time.time() < end:
        v = cond()
        if v:
            return v
        time.sleep(step)
    return cond()


def posts_since(n):
    return [p for p in POSTS[n:] if p.get('path') == '/download']


def run_browser(kind, site_base, download_dir):
    print(f'\n=== {kind} ===', flush=True)
    try:
        driver = start_browser(kind, download_dir)
    except Exception as e:  # noqa: BLE001
        check(f'{kind}: browser starts with the extension', False, str(e)[:200])
        return
    try:
        time.sleep(3)  # let the extension's background start
        hello = wait_for(lambda: [p for p in POSTS if p.get('path') == '/hello'], timeout=10)
        check(f'{kind}: the extension says hello, naming its browser', bool(hello) and hello[0].get('browser') == ('edge' if kind == 'edge' else kind) and bool(hello[0].get('version')), hello)
        driver.get(site_base + '/')
        time.sleep(1.5)

        # ---- a download the browser starts: handed over with the page's cookies
        n = len(POSTS)
        browser_download(driver, kind, site_base + '/dl/movie', 'holiday-movie.mp4')
        got = wait_for(lambda: posts_since(n), timeout=20)
        p = got[0] if got else {}
        check(f'{kind}: a browser download is sent to Nexdrop', bool(got) and '/dl/movie' in p.get('url', '') and p.get('path') == '/download', got)
        check(f'{kind}: the page login cookie travels with it', 'sid=abc123' in (p.get('cookies') or ''), p.get('cookies'))
        check(f'{kind}: the user agent is included', bool(p.get('user_agent')), p.get('user_agent'))
        check(f'{kind}: the request comes from the extension', (p.get('origin') or '').startswith(('chrome-extension://', 'moz-extension://')), p.get('origin'))
        time.sleep(2)
        left = os.listdir(download_dir)
        check(f'{kind}: the browser own download is cancelled', left == [], left)

        # ---- a PDF the browser would open in the tab
        driver.get(site_base + '/')
        time.sleep(1)
        n = len(POSTS)
        driver.find_element(By.ID, 'pdf').click()
        got = wait_for(lambda: posts_since(n))
        check(f'{kind}: clicking a PDF link sends it instead of opening it', bool(got) and got[0].get('url', '').endswith('/files/report.pdf'), got)
        check(f'{kind}: and tells Nexdrop which page it came from', bool(got) and (got[0].get('referer') or '').startswith(site_base), got and got[0].get('referer'))
        time.sleep(0.5)
        check(f'{kind}: the tab stays on the page', driver.current_url.rstrip('/') == site_base, driver.current_url)

        # ---- an ordinary page is left alone
        n = len(POSTS)
        driver.find_element(By.ID, 'page').click()
        time.sleep(1.5)
        check(f'{kind}: ordinary links are left alone', len(POSTS) == n and driver.current_url.endswith('/other.html'), (len(POSTS) - n, driver.current_url))

        # ---- Alt-click: the browser keeps it
        driver.get(site_base + '/')
        time.sleep(1)
        n = len(POSTS)
        link = driver.find_element(By.ID, 'mp4')
        ActionChains(driver).key_down(Keys.ALT).click(link).key_up(Keys.ALT).perform()
        time.sleep(2.5)
        check(f'{kind}: Alt-click leaves a link to the browser', len(POSTS) == n, posts_since(n))

        # ---- the video button: the page's stream is found
        driver.get(site_base + '/video.html')
        time.sleep(2)
        video = driver.find_element(By.ID, 'v')
        driver.execute_script('arguments[0].play().catch(()=>{})', video)
        time.sleep(1.5)
        n = len(POSTS)
        # The button sits in the video's top-right corner; click where it appears.
        size = video.size
        ActionChains(driver).move_to_element(video).pause(0.4).move_to_element_with_offset(video, size['width'] // 2 - 56, -size['height'] // 2 + 24).pause(0.3).click().perform()
        got = wait_for(lambda: posts_since(n), timeout=8)
        check(f'{kind}: the video button sends the detected stream', bool(got) and '/stream/master.m3u8' in got[0].get('url', ''), got)
        if got:
            check(f'{kind}: the video is named after the page', 'Holiday' in (got[0].get('filename') or ''), got[0].get('filename'))

        # ---- the video button on a plain video file
        driver.get(site_base + '/plainvideo.html')
        time.sleep(2)
        video = driver.find_element(By.ID, 'v')
        driver.execute_script('arguments[0].play().catch(()=>{})', video)
        time.sleep(1.5)
        n = len(POSTS)
        ActionChains(driver).move_to_element(video).pause(0.4).move_to_element_with_offset(video, size['width'] // 2 - 56, -size['height'] // 2 + 24).pause(0.3).click().perform()
        got = wait_for(lambda: posts_since(n), timeout=8)
        check(f'{kind}: the video button sends a plain video file', bool(got) and got[0].get('url', '').endswith('/files/clip.mp4'), got)

        # ---- several links: the ones in a selection, and everything on a page
        driver.get(site_base + '/links.html')
        time.sleep(1.5)
        driver.execute_script("window.getSelection().selectAllChildren(document.getElementById('picked'))")
        find_tab = "const cb = arguments[arguments.length - 1]; chrome.tabs.query({}, tabs => { const t = tabs.find(x => (x.url || '').includes('/links.html')); chrome.runtime.sendMessage({ type: 'SEND_LINKS', tabId: t.id, mode: arguments[0] }, r => cb(r)); });"
        n = len(POSTS)
        extension_call(driver, kind, find_tab, 'selection')
        got = wait_for(lambda: many_posts(n), timeout=8)
        names = sorted(u.rsplit('/', 1)[-1] for u in (got[0].get('urls', []) if got else []))
        check(f'{kind}: links in a selection are sent together', names == ['doc.pdf', 'song-one.mp3', 'song-two.mp3'], names)
        check(f'{kind}: with the page and the login it was on', bool(got) and got[0].get('referer', '').endswith('/links.html') and 'sid=abc123' in (got[0].get('cookies') or ''), got and (got[0].get('referer'), got[0].get('cookies')))
        driver.execute_script("window.getSelection().removeAllRanges()")
        n = len(POSTS)
        extension_call(driver, kind, find_tab, 'page')
        got = wait_for(lambda: many_posts(n), timeout=8)
        names = sorted(u.rsplit('/', 1)[-1] for u in (got[0].get('urls', []) if got else []))
        check(f'{kind}: grab-all sends every link on the page, without in-page anchors or mail links', names == ['other.html', 'outside.zip', 'song-one.mp3', 'song-two.mp3'], names)

        # ---- the video button stays out of the way: it hides when the pointer rests or leaves
        driver.get(site_base + '/plainvideo.html')
        time.sleep(2)
        video = driver.find_element(By.ID, 'v')
        state = lambda: driver.execute_script("var h = document.querySelector('[data-nexdrop]'); return h && h.getAttribute('data-nexdrop')")
        ActionChains(driver).move_to_element(video).perform()
        time.sleep(0.2)
        ActionChains(driver).move_by_offset(30, 20).perform()
        check(f'{kind}: the video button appears when you move over a video', wait_for(lambda: state() == 'visible', timeout=3) is True, state())
        time.sleep(4.5)  # the pointer rests
        check(f'{kind}: and goes away by itself when the pointer rests', state() == 'hidden', state())
        ActionChains(driver).move_by_offset(-20, 10).perform()
        check(f'{kind}: and comes back when the pointer moves again', wait_for(lambda: state() == 'visible', timeout=3) is True, state())
        ActionChains(driver).move_to_element(driver.find_element(By.ID, 'away')).perform()
        check(f'{kind}: and goes away when the pointer leaves the video', wait_for(lambda: state() == 'hidden', timeout=4) is True, state())

        # ---- app not running: the browser keeps its download
        global BRIDGE_UP
        bridge.shutdown(); bridge.server_close()
        BRIDGE_UP = False
        driver.get(site_base + '/')
        time.sleep(1)
        browser_download(driver, kind, site_base + '/dl/movie', 'kept-movie.mp4')
        files = wait_for(lambda: [f for f in os.listdir(download_dir) if f.startswith('kept-movie') and not f.endswith(('.crdownload', '.part', '.tmp'))], timeout=25)
        check(f'{kind}: with Nexdrop closed the browser keeps the download', bool(files), os.listdir(download_dir))
    finally:
        driver.quit()


def main():
    global bridge
    kinds = sys.argv[1:] or ['edge', 'firefox']
    out = tempfile.mkdtemp(prefix='nexdrop_ext_')
    site = Threaded(('127.0.0.1', 0), Site)
    threading.Thread(target=site.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{site.server_address[1]}'
    status = 0
    for kind in kinds:
        POSTS.clear()
        try:
            bridge = Threaded(('127.0.0.1', 18888), Bridge)
        except OSError:
            print('Port 18888 is busy (is Nexdrop running?). Close it first.')
            return 2
        threading.Thread(target=bridge.serve_forever, daemon=True).start()
        dl = os.path.join(out, kind)
        os.makedirs(dl)
        try:
            run_browser(kind, base, dl)
        finally:
            try:
                bridge.shutdown(); bridge.server_close()
            except Exception:
                pass
    shutil.rmtree(out, ignore_errors=True)
    failed = [n for n, ok in results if not ok]
    print(f'\n{len(results) - len(failed)}/{len(results)} checks passed')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
