"""End-to-end checks of 1.4 features in the REAL app: refresh link, replace an existing file,
the activity log, the existing-file check and the proxy test.

    python scripts/e2e_features.py

Uses your real Nexdrop database (backed up first, restored afterwards). Close Nexdrop first.
"""
import http.server, json, os, shutil, socketserver, subprocess, sys, tempfile, threading, time, urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'scripts'))
from e2e_torrent import WS, App

EXE = os.path.join(ROOT, 'src-tauri', 'target', 'release', 'nexdrop.exe')
DB = os.path.join(os.environ['LOCALAPPDATA'], 'Nexdrop', 'nexdrop.db')
BACKUP = os.path.join(tempfile.gettempdir(), 'nexdrop_features_e2e_backup')
DEBUG_PORT = 9229
FILES = {}
DEAD = set()  # paths that answer 403 (an expired link)
results = []


def check(name, ok, detail=''):
    results.append((name, ok))
    print(('PASS  ' if ok else 'FAIL  ') + name + (f'  [{detail}]' if detail and not ok else ''), flush=True)


def blob(n, seed):
    return bytes((i * 31 + seed * 7 + 5) % 251 for i in range(n))


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *a):
        pass

    def _go(self, head):
        path = self.path.split('?')[0].lstrip('/')
        name = path.split('/')[-1]
        data = FILES.get(name)
        if data is None or path in DEAD:
            self.send_response(403 if path in DEAD else 404)
            self.send_header('Content-Length', '0')
            self.end_headers()
            return
        start, end = 0, len(data) - 1
        rng = self.headers.get('Range')
        if rng and rng.startswith('bytes=') and not head:
            a, _, b = rng[6:].partition('-')
            start = int(a or 0)
            end = min(int(b), len(data) - 1) if b else len(data) - 1
            self.send_response(206)
            self.send_header('Content-Range', f'bytes {start}-{end}/{len(data)}')
        else:
            self.send_response(200)
        self.send_header('Content-Length', str(end - start + 1))
        self.send_header('Accept-Ranges', 'bytes')
        self.send_header('Content-Type', 'video/mp4')
        self.end_headers()
        if head:
            return
        try:
            sent, t0 = start, time.time()
            slow = 'slow' in path
            while sent <= end:
                n = min(64 * 1024, end - sent + 1)
                self.wfile.write(data[sent:sent + n])
                sent += n
                if slow:
                    time.sleep(0.05)
        except Exception:
            pass

    def do_HEAD(self):
        self._go(True)

    def do_GET(self):
        self._go(False)


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


def wait(cond, timeout=30, step=0.25):
    end = time.time() + timeout
    while time.time() < end:
        v = cond()
        if v:
            return v
        time.sleep(step)
    return cond()


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

    FILES['movie.mp4'] = blob(6_000_000, 1)
    FILES['other.mp4'] = blob(3_000_000, 2)
    FILES['clip.mp4'] = blob(900_000, 3)
    server = Server(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_address[1]}'
    dest = tempfile.mkdtemp(prefix='nexdrop_feat_dest_')

    env = dict(os.environ, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=f'--remote-debugging-port={DEBUG_PORT}')
    app = subprocess.Popen([EXE], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        page = None
        for _ in range(60):
            try:
                pages = json.loads(urllib.request.urlopen(f'http://127.0.0.1:{DEBUG_PORT}/json', timeout=1).read())
                page = next(p for p in pages if p['type'] == 'page' and 'tauri.localhost' in p['url'])
                break
            except Exception:
                time.sleep(0.5)
        ws = WS(page['webSocketDebuggerUrl'])
        ws.call('Runtime.enable')
        time.sleep(2)
        ui = App(ws)
        task = lambda i: ui.invoke('get_download_cmd', {'id': i}).get('ok') or {}
        log = lambda i: [l['message'] for l in (ui.invoke('get_download_log_cmd', {'id': i}).get('ok') or [])]

        # ---- refresh link: a slow download whose link "expires", continued with a new link
        old = f'{base}/a/slow/movie.mp4'
        r = ui.invoke('add_download_cmd', {'req': {'url': old, 'destination_path': dest, 'connections': 2}})
        t = r.get('ok') or {}
        check('refresh: download added', bool(t.get('id')), r)
        tid = t.get('id')
        wait(lambda: task(tid).get('downloaded_bytes', 0) > 500_000, 20)
        ui.invoke('pause_download_cmd', {'id': tid})
        wait(lambda: task(tid).get('status') == 'paused', 10)
        before = task(tid).get('downloaded_bytes', 0)
        DEAD.add('a/slow/movie.mp4')
        bad = ui.invoke('refresh_link_cmd', {'id': tid, 'url': f'{base}/b/other.mp4'})
        check('refresh: a link to a different file is refused', 'err' in bad and 'different file' in bad.get('err', ''), bad)
        good = ui.invoke('refresh_link_cmd', {'id': tid, 'url': f'{base}/b/movie.mp4'})
        check('refresh: the same file from a new link is accepted', 'ok' in good, good)
        done = wait(lambda: task(tid).get('status') == 'completed', 60)
        t = task(tid)
        p = os.path.join(t.get('destination_path', ''), t.get('filename', ''))
        check('refresh: it finishes from the new link', bool(done) and t.get('url', '').endswith('/b/movie.mp4'), t.get('status'))
        check('refresh: the file is exact', os.path.exists(p) and open(p, 'rb').read() == FILES['movie.mp4'])
        check('refresh: progress was kept (did not start over)', before > 0 and t.get('downloaded_bytes') == len(FILES['movie.mp4']), before)
        lines = log(tid)
        check('log: start, pause and the link change are recorded',
              any(l.startswith('Started') for l in lines) and any(l.startswith('Paused') for l in lines) and any('Link refreshed' in l for l in lines), lines)

        # ---- existing file: detected; "Replace it" overwrites instead of making "(1)"
        target_dir = os.path.join(dest, 'Videos')
        os.makedirs(target_dir, exist_ok=True)
        open(os.path.join(target_dir, 'clip.mp4'), 'wb').write(b'old content')
        found = ui.invoke('existing_file_cmd', {'destination': dest, 'filename': 'clip.mp4', 'category': 'videos'}).get('ok')
        check('existing file: found where the download would go', bool(found) and found.lower().endswith(os.path.join('videos', 'clip.mp4').lower()), found)
        none = ui.invoke('existing_file_cmd', {'destination': dest, 'filename': 'nothing.mp4', 'category': 'videos'})
        check('existing file: nothing reported when there is none', none.get('ok') is None and 'err' not in none, none)
        r = ui.invoke('add_download_cmd', {'req': {'url': f'{base}/c/clip.mp4', 'destination_path': dest, 'overwrite': True}})
        cid = (r.get('ok') or {}).get('id')
        wait(lambda: task(cid).get('status') == 'completed', 30)
        names = sorted(n for n in os.listdir(target_dir) if n.startswith('clip'))
        check('replace: the old file is replaced, no "(1)" copy', names == ['clip.mp4'] and open(os.path.join(target_dir, 'clip.mp4'), 'rb').read() == FILES['clip.mp4'], names)
        open(os.path.join(target_dir, 'clip.mp4'), 'ab').write(b'')
        r = ui.invoke('add_download_cmd', {'req': {'url': f'{base}/d/clip.mp4', 'destination_path': dest}})
        kid = (r.get('ok') or {}).get('id')
        wait(lambda: task(kid).get('status') == 'completed', 30)
        names = sorted(n for n in os.listdir(target_dir) if n.startswith('clip'))
        check('keep both: a second copy gets its own name', names == ['clip (1).mp4', 'clip.mp4'], names)

        # ---- proxy test: a dead proxy reports an error instead of hanging
        t0 = time.time()
        pt = ui.invoke('test_proxy_cmd', {'mode': 'custom', 'url': 'socks5://127.0.0.1:9', 'username': '', 'password': ''}).get('ok') or {}
        check('proxy test: a dead proxy fails quickly with a reason', pt.get('ok') is False and bool(pt.get('error')) and time.time() - t0 < 20, pt)
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

    failed = [n for n, passed in results if not passed]
    print(f'\n{len(results) - len(failed)}/{len(results)} checks passed')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
