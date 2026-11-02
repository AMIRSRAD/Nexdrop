"""End-to-end check of adding many downloads at once, in the REAL app.

    python scripts/e2e_bulk.py

Starts a local server whose answers are slow (like a busy site), launches the built app
and adds 22 links in one go through the same command the "Add multiple" window uses:
20 good ones and 2 that do not exist. Checks that the command returns immediately, that the
downloads show up in the list quickly and in the order given (not one after another), that
the two bad links are reported, and that every file arrives byte for byte.

Uses your real Nexdrop database (backed up first, restored afterwards). Close Nexdrop first.
"""
import http.server, json, os, shutil, socketserver, subprocess, sys, tempfile, threading, time, urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'scripts'))
from e2e_torrent import WS, App

EXE = os.path.join(ROOT, 'src-tauri', 'target', 'release', 'nexdrop.exe')
DATA = os.path.join(os.environ['LOCALAPPDATA'], 'Nexdrop')
DB = os.path.join(DATA, 'nexdrop.db')
BACKUP = os.path.join(tempfile.gettempdir(), 'nexdrop_bulk_e2e_backup')
DEBUG_PORT = 9228
DELAY = 0.8  # every answer is this slow
FILES = {}
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

    def _send(self, head_only):
        time.sleep(DELAY)
        name = self.path.split('?')[0].lstrip('/')
        data = FILES.get(name)
        if data is None:
            self.send_response(404)
            self.send_header('Content-Length', '0')
            self.end_headers()
            return
        start, end = 0, len(data) - 1
        rng = self.headers.get('Range')
        if rng and rng.startswith('bytes=') and not head_only:
            a, _, b = rng[6:].partition('-')
            start = int(a or 0)
            end = min(int(b), len(data) - 1) if b else len(data) - 1
            self.send_response(206)
            self.send_header('Content-Range', f'bytes {start}-{end}/{len(data)}')
        else:
            self.send_response(200)
        self.send_header('Content-Length', str(end - start + 1))
        self.send_header('Accept-Ranges', 'bytes')
        self.end_headers()
        if not head_only:
            self.wfile.write(data[start:end + 1])

    def do_HEAD(self):
        self._send(True)

    def do_GET(self):
        self._send(False)


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


def wait(cond, timeout=30, step=0.2):
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

    names = []
    for i in range(20):
        n = f'bulk{i:02d}.mp4'
        FILES[n] = blob(300_000 + i * 90_000, i)
        names.append(n)
    server = Server(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_address[1]}'
    order = names[:5] + ['missing1.mp4'] + names[5:12] + ['missing2.mp4'] + names[12:]
    dest = tempfile.mkdtemp(prefix='nexdrop_bulk_dest_')

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
        if not page:
            print('could not attach to the app webview')
            return 1
        ws = WS(page['webSocketDebuggerUrl'])
        ws.call('Runtime.enable')
        time.sleep(2)
        ui = App(ws)

        # listen for the summary event the interface uses for its toast
        ws.call('Runtime.evaluate', {'expression': """
            window.__bf = null;
            const h = window.__TAURI_INTERNALS__.transformCallback((e) => { window.__bf = e.payload; });
            window.__TAURI_INTERNALS__.invoke('plugin:event|listen', {event: 'batch-add-finished', target: {kind: 'Any'}, handler: h});
            true""", 'returnByValue': True})

        def downloads():
            return ui.invoke('get_all_downloads_cmd').get('ok') or []

        before = len(downloads())
        reqs = [{'url': f'{base}/{n}', 'destination_path': dest, 'connections': 2} for n in order]
        t0 = time.time()
        r = ui.invoke('batch_add_downloads_cmd', {'reqs': reqs})
        took = time.time() - t0
        check('bulk: the command returns at once', 'ok' in r and took < 1.5, f'{took:.2f}s {r}')

        mine = lambda: [d for d in downloads() if d['url'].startswith(base)]
        first = wait(lambda: len(mine()) >= 1, 5)
        check('bulk: the first download shows within a few seconds', bool(first), len(mine()))
        listed = wait(lambda: len(mine()) == 20, 12)
        t_listed = time.time() - t0
        sequential = len(order) * DELAY * 2
        check('bulk: all 20 are listed far sooner than one-by-one (%.0fs)' % sequential, bool(listed) and t_listed < sequential / 2, f'{t_listed:.1f}s, {len(mine())} listed')

        fin = wait(lambda: ui.ws.call('Runtime.evaluate', {'expression': 'window.__bf', 'returnByValue': True}).get('result', {}).get('result', {}).get('value'), 20)
        check('bulk: the summary says 20 added', bool(fin) and fin.get('added') == 20, fin)
        bad_urls = sorted(f['url'].rsplit('/', 1)[1] for f in (fin or {}).get('failed', []))
        check('bulk: the two missing links are reported by address', bad_urls == ['missing1.mp4', 'missing2.mp4'], bad_urls)
        check('bulk: failures carry a reason', all(f.get('error') for f in (fin or {}).get('failed', [])), fin)

        listing = sorted(mine(), key=lambda d: d['created_at'])
        got_order = [d['url'].rsplit('/', 1)[1] for d in listing]
        check('bulk: the order of the list is the order given', got_order == names, got_order[:6])

        done = wait(lambda: all(d['status'] == 'completed' for d in mine()) and len(mine()) == 20, 90, 0.5)
        check('bulk: every download completes', bool(done), [(d['filename'], d['status']) for d in mine() if d['status'] != 'completed'][:5])
        exact = True
        for d in mine():
            p = os.path.join(d['destination_path'], d['filename'])
            name = d['url'].rsplit('/', 1)[1]
            if not (os.path.exists(p) and open(p, 'rb').read() == FILES[name]):
                exact = False
                print('   mismatch:', name)
        check('bulk: every file is byte for byte right', exact)
        check('bulk: nothing else was added', len(downloads()) - before == 20, len(downloads()) - before)
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
