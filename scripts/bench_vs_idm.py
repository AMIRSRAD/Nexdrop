"""Head-to-head download timing: Nexdrop vs Internet Download Manager, same server, same file.

    python scripts/bench_vs_idm.py [size_mb] [per_connection_mib_s]

A local HTTP server serves one video file with Range support and a speed cap on every
connection (like a CDN that limits each connection), so what is measured is how well each
manager uses parallel connections. Each manager downloads the file; we time from the moment
it is told to start until the finished file is on disk with the right size and hash.

Close Nexdrop first (it is launched here with a debugging port). Uses your real Nexdrop
database; it is backed up first and restored afterwards. IDM is driven by its command line.
"""
import hashlib, http.server, json, os, shutil, socketserver, subprocess, sys, tempfile, threading, time, urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'scripts'))
from e2e_torrent import WS, App

EXE = os.path.join(ROOT, 'src-tauri', 'target', 'release', 'nexdrop.exe')
IDM = r'C:\Program Files (x86)\Internet Download Manager\IDMan.exe'
DATA = os.path.join(os.environ['LOCALAPPDATA'], 'Nexdrop')
DB = os.path.join(DATA, 'nexdrop.db')
BACKUP = os.path.join(tempfile.gettempdir(), 'nexdrop_bench_backup')
DEBUG_PORT = 9227
MB = 1024 * 1024
SIZE = int(sys.argv[1]) * MB if len(sys.argv) > 1 else 120 * MB
CAP = int(float(sys.argv[2]) * MB) if len(sys.argv) > 2 else 2 * MB
PATTERN = hashlib.sha256(b'nexdrop-bench').digest() * 2048  # 64 KiB block


def block(offset, n):
    start = offset % len(PATTERN)
    out = (PATTERN[start:] + PATTERN * (n // len(PATTERN) + 2))[:n]
    return out


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *a):
        pass

    def head(self, code, length, extra=()):
        self.send_response(code)
        self.send_header('Content-Type', 'video/mp4')
        self.send_header('Content-Disposition', 'attachment; filename="bench.mp4"')
        self.send_header('Accept-Ranges', 'bytes')
        self.send_header('Content-Length', str(length))
        for k, v in extra:
            self.send_header(k, v)
        self.end_headers()

    def do_HEAD(self):
        self.head(200, SIZE)

    def do_GET(self):
        start, end = 0, SIZE - 1
        rng = self.headers.get('Range')
        if rng and rng.startswith('bytes='):
            a, _, b = rng[6:].partition('-')
            start = int(a or 0)
            end = min(int(b), SIZE - 1) if b else SIZE - 1
            self.head(206, end - start + 1, [('Content-Range', f'bytes {start}-{end}/{SIZE}')])
        else:
            self.head(200, SIZE)
        try:
            sent, t0, piece = start, time.time(), 64 * 1024
            while sent <= end:
                n = min(piece, end - sent + 1)
                self.wfile.write(block(sent, n))
                sent += n
                due = (sent - start) / CAP
                el = time.time() - t0
                if due > el:
                    time.sleep(due - el)
        except Exception:
            pass


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


def expected_hash():
    h = hashlib.sha256()
    pos = 0
    while pos < SIZE:
        n = min(4 * MB, SIZE - pos)
        h.update(block(pos, n))
        pos += n
    return h.hexdigest()


def sha(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        while chunk := f.read(4 * MB):
            h.update(chunk)
    return h.hexdigest()


def wait_file(path, want_hash, timeout):
    t0 = time.time()
    while time.time() - t0 < timeout:
        if os.path.exists(path) and os.path.getsize(path) == SIZE:
            time.sleep(0.2)
            try:
                if sha(path) == want_hash:
                    return time.time()
            except OSError:
                pass
        time.sleep(0.1)
    return None


def main():
    try:
        urllib.request.urlopen('http://127.0.0.1:18888/status', timeout=1)
        print('Nexdrop is running; close it first.')
        return 2
    except Exception:
        pass
    if not os.path.exists(IDM):
        print('IDM not found')
        return 2

    os.makedirs(BACKUP, exist_ok=True)
    for ext in ('', '-wal', '-shm'):
        if os.path.exists(DB + ext):
            shutil.copy2(DB + ext, os.path.join(BACKUP, 'nexdrop.db' + ext))

    server = Server(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_address[1]}'
    want = expected_hash()
    ideal = SIZE / CAP
    print(f'file {SIZE // MB} MB, each connection capped at {CAP / MB:g} MiB/s', flush=True)

    out = tempfile.mkdtemp(prefix='bench_')
    results = {'Nexdrop': [], 'IDM': []}
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
        settings = (ui.invoke('get_settings_cmd') or {}).get('ok') or {}
        print('Nexdrop connections per download:', settings.get('default_connections'), flush=True)

        for rnd in range(2):
            # Nexdrop
            dest = os.path.join(out, f'nx{rnd}')
            os.makedirs(dest)
            t0 = time.time()
            r = ui.invoke('add_download_cmd', {'req': {'url': base + '/bench.mp4', 'filename': 'bench.mp4', 'destination_path': dest}})
            print('add:', str(r)[:300], flush=True)
            done = wait_file(os.path.join(dest, "Videos", "bench.mp4"), want, 150)
            results['Nexdrop'].append(done - t0 if done else None)
            print(f'round {rnd + 1} Nexdrop: {results["Nexdrop"][-1]}', flush=True)
            time.sleep(2)

            # IDM
            dest = os.path.join(out, f'idm{rnd}')
            os.makedirs(dest)
            t0 = time.time()
            subprocess.Popen([IDM, '/d', base + '/bench.mp4', '/p', dest, '/f', 'bench.mp4', '/n'])
            done = wait_file(os.path.join(dest, "bench.mp4"), want, 150)
            results['IDM'].append(done - t0 if done else None)
            print(f'round {rnd + 1} IDM: {results["IDM"][-1]}', flush=True)
            time.sleep(2)
    finally:
        app.terminate()
        try:
            app.wait(10)
        except Exception:
            app.kill()
        time.sleep(1)
        for ext in ('', '-wal', '-shm'):
            src = os.path.join(BACKUP, 'nexdrop.db' + ext)
            if os.path.exists(src):
                shutil.copy2(src, DB + ext)
        shutil.rmtree(out, ignore_errors=True)

    print(json.dumps({'size_mb': SIZE // MB, 'cap_mib_s': CAP / MB, 'ideal_secs_one_connection': ideal, 'results': results}))
    return 0


if __name__ == '__main__':
    sys.exit(main())
