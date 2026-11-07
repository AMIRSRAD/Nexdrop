"""'Find the download' in the REAL app, on a local site that behaves like a download page.

    python scripts/e2e_resolver.py

Pages: a countdown button that asks for a temporary link, with a promo "Install our app" button
that must be left alone; a two-step page; a page whose button isn't recognisable (the app must
show its view so a person can finish); a page with nothing to find (cancel). The temporary
links need the page's cookie and referer, like real file hosts, and the final download goes
through the engine with them. Close Nexdrop first. Uses your real database (backed up, restored).
"""
import http.server, json, os, shutil, socketserver, subprocess, sys, tempfile, threading, time, urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'scripts'))
from e2e_torrent import WS, App

EXE = os.path.join(ROOT, 'src-tauri', 'target', 'release', 'nexdrop.exe')
DB = os.path.join(os.environ['LOCALAPPDATA'], 'Nexdrop', 'nexdrop.db')
BACKUP = os.path.join(tempfile.gettempdir(), 'nexdrop_resolver_e2e_backup')
DEBUG_PORT = 9233
DATA = bytes((i * 31 + 7) % 251 for i in range(1_500_000))
CLICKED_PROMO = []
results = []


def check(name, ok, detail=''):
    results.append((name, ok))
    print(('PASS  ' if ok else 'FAIL  ') + name + (f'  [{detail}]' if detail and not ok else ''), flush=True)


PAGES = {
    # a countdown button asks the site for a temporary link, like real file hosts do
    '/file/one': """<!doctype html><title>Download movie.mp4</title>
      <a id="promo" class="btn" href="/promo/app">Install our app</a>
      <a id="download-btn" class="btn disabled" href="#">Download</a>
      <script>
        const b = document.getElementById('download-btn');
        setTimeout(() => b.classList.remove('disabled'), 2500);
        b.addEventListener('click', async (e) => { e.preventDefault(); if (b.classList.contains('disabled')) return;
          const r = await fetch('/api/sign?id=one'); const j = await r.json(); location.href = j.url; });
      </script>""",
    '/file/two': """<!doctype html><title>Step 1</title><p>Your file is ready.</p>
      <a class="go" href="/file/two/next">Download</a>""",
    '/file/two/next': """<!doctype html><title>Step 2</title><p>Almost there.</p>
      <button id="get" onclick="location.href='/dl/signed/two'">Download file</button>""",
    # nothing a script could recognise: a person has to press it
    '/file/three': """<!doctype html><title>Please wait</title><p>Press the button when you are ready.</p>
      <button id="go" style="padding:20px">Continue</button>
      <script>setTimeout(() => { location.href = '/dl/signed/three'; }, 15000);</script>""",
    '/file/empty': """<!doctype html><title>Nothing here</title><p>Just text.</p>""",
}


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *a):
        pass

    def reply(self, code, ctype, body, extra=None):
        self.send_response(code)
        self.send_header('Content-Type', ctype)
        self.send_header('Content-Length', str(len(body)))
        for k, v in (extra or {}).items():
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(body)

    def _go(self, head):
        path = self.path.split('?')[0]
        if path in PAGES:
            return self.reply(200, 'text/html; charset=utf-8', PAGES[path].encode(), {'Set-Cookie': 'sid=abc123; Path=/'})
        if path == '/promo/app':
            CLICKED_PROMO.append(1)
            return self.reply(200, 'text/html', b'<title>app</title>promo')
        if path == '/api/sign':
            return self.reply(200, 'application/json', json.dumps({'url': '/dl/signed/one'}).encode())
        if path.startswith('/dl/signed/'):
            # a temporary link that needs the page's cookie and referer
            if 'sid=abc123' not in (self.headers.get('Cookie') or '') or '/file/' not in (self.headers.get('Referer') or ''):
                return self.reply(403, 'text/plain', b'need the page cookie and referer')
            data = DATA
            start, end = 0, len(data) - 1
            rng = self.headers.get('Range')
            extra = {'Content-Disposition': f'attachment; filename="{path.rsplit("/", 1)[-1]}.mp4"', 'Accept-Ranges': 'bytes'}
            if rng and rng.startswith('bytes=') and not head:
                a, _, b = rng[6:].partition('-')
                start = int(a or 0); end = min(int(b), len(data) - 1) if b else len(data) - 1
                extra['Content-Range'] = f'bytes {start}-{end}/{len(data)}'
                return self.reply(206, 'video/mp4', data[start:end + 1], extra)
            if head:
                self.send_response(200); self.send_header('Content-Type', 'video/mp4'); self.send_header('Content-Length', str(len(data)))
                for k, v in extra.items():
                    self.send_header(k, v)
                self.end_headers(); return
            return self.reply(200, 'video/mp4', data, extra)
        self.reply(404, 'text/plain', b'nope')

    def do_HEAD(self):
        self._go(True)

    def do_GET(self):
        self._go(False)


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


def cdp_pages():
    return json.loads(urllib.request.urlopen(f'http://127.0.0.1:{DEBUG_PORT}/json', timeout=2).read())


def main():
    try:
        urllib.request.urlopen('http://127.0.0.1:18888/status', timeout=1)
        print('A Nexdrop instance is already running; close it first.')
        return 2
    except Exception:
        pass
    os.makedirs(BACKUP, exist_ok=True)
    for ext in ('', '-wal', '-shm'):
        if os.path.exists(DB + ext):
            shutil.copy2(DB + ext, os.path.join(BACKUP, 'nexdrop.db' + ext))
    server = Server(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_address[1]}'
    dest = tempfile.mkdtemp(prefix='nexdrop_resolver_dest_')
    env = dict(os.environ, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=f'--remote-debugging-port={DEBUG_PORT}')
    app = subprocess.Popen([EXE], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        page = None
        for _ in range(60):
            try:
                page = next(p for p in cdp_pages() if p['type'] == 'page' and 'tauri.localhost' in p['url'])
                break
            except Exception:
                time.sleep(0.5)
        ui = App(WS(page['webSocketDebuggerUrl']))
        ui.ws.call('Runtime.enable')
        time.sleep(2)
        resolve = lambda url: ui.invoke('resolve_page_cmd', {'url': url})

        # ---- 1: countdown button, promo ignored, signed link needs cookie + referer
        t0 = time.time()
        r = resolve(f'{base}/file/one')
        got = r.get('ok') or {}
        check('one: the page\'s download is found', bool(got) and '/dl/signed/one' in got.get('url', ''), r)
        check('one: it waited out the countdown (button disabled for ~2.5 s)', time.time() - t0 >= 2.4, f'{time.time() - t0:.1f}s')
        check('one: the "Install our app" button was left alone', not CLICKED_PROMO, CLICKED_PROMO)
        check('one: no window was needed', got.get('needed_user') is False, got)
        check('one: the page\'s cookie comes along', 'sid=abc123' in (got.get('cookie') or ''), got.get('cookie'))
        check('one: and the page as referer', (got.get('referer') or '').startswith(base + '/file/one'), got.get('referer'))
        time.sleep(0.5)
        check('one: the helper window is gone afterwards', not [p for p in cdp_pages() if '/file/' in p.get('url', '')], [p['url'] for p in cdp_pages()])

        # the engine can then download it with those, and the file is exact
        req = {'url': got['url'], 'destination_path': dest, 'connections': 2, 'referer': got['referer'], 'user_agent': got['user_agent'], 'custom_headers': {'Cookie': got['cookie']}}
        t = (ui.invoke('add_download_cmd', {'req': req}).get('ok') or {})
        deadline = time.time() + 30
        while time.time() < deadline and (ui.invoke('get_download_cmd', {'id': t.get('id')}).get('ok') or {}).get('status') != 'completed':
            time.sleep(0.4)
        done = ui.invoke('get_download_cmd', {'id': t.get('id')}).get('ok') or {}
        p = os.path.join(done.get('destination_path', ''), done.get('filename', ''))
        check('one: the file downloads with those and is exact', done.get('status') == 'completed' and os.path.exists(p) and open(p, 'rb').read() == DATA, (done.get('status'), done.get('filename')))
        without = ui.invoke('add_download_cmd', {'req': {'url': got['url'], 'destination_path': dest, 'connections': 1}})
        check('one: and the link alone (no cookie, no referer) is refused, as on real hosts', 'err' in without, without)

        # ---- 2: two steps
        r = resolve(f'{base}/file/two')
        got = r.get('ok') or {}
        check('two: it follows the page to its second step and finds the download', '/dl/signed/two' in got.get('url', ''), r)
        check('two: the referer is the second page', (got.get('referer') or '').endswith('/file/two/next'), got.get('referer'))

        # ---- 3: nothing recognisable: the app shows its window; the "person" is the page's own timer
        t0 = time.time()
        r = resolve(f'{base}/file/three')
        got = r.get('ok') or {}
        check('three: a page with no recognisable button is still caught once it starts a download', '/dl/signed/three' in got.get('url', ''), r)
        check('three: it kept the window hidden first, then showed it for the person', got.get('needed_user') is True and time.time() - t0 >= 12, (got.get('needed_user'), f'{time.time() - t0:.1f}s'))

        # ---- 4: nothing to find, cancelled by the user
        res4 = {}
        th = threading.Thread(target=lambda: res4.update(r=resolve(f'{base}/file/empty')))
        th.start()
        time.sleep(2)
        c = App(WS(next(p for p in cdp_pages() if p['type'] == 'page' and 'tauri.localhost' in p['url'])['webSocketDebuggerUrl']))
        c.ws.call('Runtime.enable')
        c.invoke('cancel_resolve_cmd')
        th.join(15)
        check('four: cancelling stops the search', 'Cancelled' in json.dumps(res4.get('r')), res4)
        time.sleep(0.5)
        check('four: and closes the window', not [p for p in cdp_pages() if '/file/empty' in p.get('url', '')], [p['url'] for p in cdp_pages()])
        again = resolve(f'{base}/file/one')
        check('four: another search can start right after', 'ok' in again, again)
    finally:
        app.kill()
        time.sleep(1)
        for ext in ('', '-wal', '-shm'):
            try:
                os.remove(DB + ext)
            except OSError:
                pass
            b = os.path.join(BACKUP, 'nexdrop.db' + ext)
            if os.path.exists(b):
                shutil.copy2(b, DB + ext)
        shutil.rmtree(dest, ignore_errors=True)
        print('database restored', flush=True)
    failed = [n for n, ok in results if not ok]
    print(f'\n{len(results) - len(failed)}/{len(results)} checks passed')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
