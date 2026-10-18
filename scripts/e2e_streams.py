"""End-to-end check of streaming video (HLS) in the REAL app.

    pnpm build && cargo build --release --manifest-path src-tauri/Cargo.toml
    python scripts/e2e_streams.py

Serves a small HLS site locally, launches the built app with a WebView2
debugging port and drives the real Tauri commands: probing, choosing a
quality, downloading, pause/resume, resuming after the app is killed
mid-download, removing partial files and the browser bridge route.

It uses your real Nexdrop database, so the database is backed up first and
restored afterwards. Downloads go to a temporary folder. Close Nexdrop first.
"""
import http.server, json, os, shutil, socketserver, subprocess, sys, tempfile, threading, time, urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'scripts'))
from e2e_torrent import WS, App  # tiny CDP client

EXE = os.path.join(ROOT, 'src-tauri', 'target', 'release', 'nexdrop.exe')
DATA = os.path.join(os.environ['LOCALAPPDATA'], 'Nexdrop')
DB = os.path.join(DATA, 'nexdrop.db')
BACKUP = os.path.join(tempfile.gettempdir(), 'nexdrop_streams_e2e_backup')
DEBUG_PORT = 9227
BRIDGE = 'http://127.0.0.1:18888'
results = []


def check(name, ok, detail=''):
    results.append((name, ok))
    print(('PASS  ' if ok else 'FAIL  ') + name + (f'  [{detail}]' if detail and not ok else ''), flush=True)


def blob(n, seed):
    return bytes((i * 31 + seed * 7 + 5) % 251 for i in range(n))


SITE = {}
DELAYS = {}  # path prefix -> seconds


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *a):
        pass

    def do_GET(self):
        path = self.path.split('?')[0]
        for prefix, delay in DELAYS.items():
            if path.startswith(prefix):
                time.sleep(delay)
        data = SITE.get(path)
        if data is None:
            self.send_response(404); self.send_header('Content-Length', '0'); self.end_headers(); return
        ctype = 'application/vnd.apple.mpegurl' if path.endswith('.m3u8') else 'video/mp2t'
        self.send_response(200)
        self.send_header('Content-Type', ctype)
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        try:
            self.wfile.write(data)
        except Exception:
            pass

    do_HEAD = do_GET


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


def playlist(names, endlist=True):
    s = '#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n'
    for n in names:
        s += f'#EXTINF:4.0,\n{n}\n'
    if endlist:
        s += '#EXT-X-ENDLIST\n'
    return s.encode()


def bridge(path, body):
    req = urllib.request.Request(BRIDGE + path, data=json.dumps(body).encode(), method='POST')
    req.add_header('Origin', 'chrome-extension://e2e')
    req.add_header('Content-Type', 'application/json')
    with urllib.request.urlopen(req, timeout=20) as r:
        return r.status, json.loads(r.read() or b'{}')


def wait(cond, timeout=30, step=0.25):
    end = time.time() + timeout
    while time.time() < end:
        v = cond()
        if v:
            return v
        time.sleep(step)
    return cond()


def launch(env):
    return subprocess.Popen([EXE], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def attach():
    for _ in range(60):
        try:
            pages = json.loads(urllib.request.urlopen(f'http://127.0.0.1:{DEBUG_PORT}/json', timeout=1).read())
            page = next(p for p in pages if p['type'] == 'page' and 'tauri.localhost' in p['url'])
            ws = WS(page['webSocketDebuggerUrl'])
            ws.call('Runtime.enable')
            time.sleep(2)
            return App(ws)
        except Exception:
            time.sleep(0.5)
    return None


def main():
    try:
        urllib.request.urlopen(BRIDGE + '/status', timeout=1)
        print('A Nexdrop instance is already running on :18888; close it first.')
        return 2
    except Exception:
        pass
    if not os.path.exists(EXE):
        print('Build the release app first: cargo build --release --manifest-path src-tauri/Cargo.toml')
        return 2

    os.makedirs(BACKUP, exist_ok=True)
    for ext in ('', '-wal', '-shm'):
        if os.path.exists(DB + ext):
            shutil.copy2(DB + ext, os.path.join(BACKUP, 'nexdrop.db' + ext))
    print('database backed up', flush=True)

    # ---- the site: master with HD/SD, a slow 120-segment stream, a live playlist
    hd = [blob(60_000 + i * 13, i) for i in range(20)]
    sd = [blob(20_000 + i * 7, 100 + i) for i in range(20)]
    SITE['/show/master.m3u8'] = b'#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1920x1080\nhd/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=600000,RESOLUTION=640x360\nsd/index.m3u8\n'
    for q, segs in (('hd', hd), ('sd', sd)):
        SITE[f'/show/{q}/index.m3u8'] = playlist([f's{i}.ts' for i in range(len(segs))])
        for i, d in enumerate(segs):
            SITE[f'/show/{q}/s{i}.ts'] = d
    slow = [blob(40_000, 200 + i % 50) for i in range(120)]
    SITE['/slow/index.m3u8'] = playlist([f's{i}.ts' for i in range(len(slow))])
    for i, d in enumerate(slow):
        SITE[f'/slow/s{i}.ts'] = d
    SITE['/live/index.m3u8'] = playlist(['a.ts', 'b.ts'], endlist=False)
    SITE['/live/a.ts'] = blob(1000, 1)
    SITE['/live/b.ts'] = blob(1000, 2)
    server = Server(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_address[1]}'

    dest = tempfile.mkdtemp(prefix='nexdrop_stream_dest_')
    env = dict(os.environ, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=f'--remote-debugging-port={DEBUG_PORT}')
    app = launch(env)
    try:
        ui = attach()
        if not ui:
            print('could not attach to the app webview')
            return 1

        def ok(r):
            return r.get('ok')

        def task(i):
            return ok(ui.invoke('get_download_cmd', {'id': i}))

        def add(url, **extra):
            r = ui.invoke('add_download_cmd', {'req': {'url': url, 'destination_path': dest, **extra}})
            return r.get('ok'), r

        def same(t, expected):
            p = os.path.join(t['destination_path'], t['filename'])
            return os.path.exists(p) and open(p, 'rb').read() == expected

        def partials():
            return [n for n in os.listdir(dest) if '.nexpart' in n]

        # ---- probe
        r = ui.invoke('probe_stream_cmd', {'url': f'{base}/show/master.m3u8', 'referer': None, 'userAgent': None})
        info = ok(r)
        check('probe: qualities listed best first', bool(info) and [v['height'] for v in info['variants']] == [1080, 360] and info['kind'] == 'hls', r)
        check('probe: title, file name and length', info['title'] == 'show' and info['filename'] == 'show.ts' and abs(info['duration_secs'] - 80) < 1, info)
        live = ok(ui.invoke('probe_stream_cmd', {'url': f'{base}/live/index.m3u8', 'referer': None, 'userAgent': None}))
        check('probe: a live stream says why it cannot be saved', bool(live) and 'Live' in (live.get('unsupported') or ''), live)
        bad = ui.invoke('probe_stream_cmd', {'url': f'{base}/nothing.m3u8', 'referer': None, 'userAgent': None})
        check('probe: a missing playlist is an error', 'err' in bad, bad)

        # ---- download the chosen quality
        t1, r = add(f'{base}/show/master.m3u8', stream_variant='600000:640x360')
        check('add: a stream is queued as a stream', bool(t1) and t1['kind'] == 'stream' and t1['filename'] == 'show.ts', r)
        done = wait(lambda: (task(t1['id']) or {}).get('status') == 'completed', 30)
        check('download: the chosen quality, byte for byte', bool(done) and same(task(t1['id']), b''.join(sd)), task(t1['id']) and task(t1['id'])['status'])
        check('download: no temporary files left', partials() == [], partials())

        # ---- no quality given: the best one
        t2, r = add(f'{base}/show/master.m3u8', filename='best')
        wait(lambda: (task(t2['id']) or {}).get('status') == 'completed', 30)
        check('download: without a choice the best quality is used', same(task(t2['id']), b''.join(hd)), task(t2['id']))

        # ---- live refused at add time
        t3, r = add(f'{base}/live/index.m3u8')
        check('add: a live stream is refused with a reason', t3 is None and 'Live' in json.dumps(r), r)

        # ---- pause and resume
        DELAYS['/slow/'] = 0.4
        t4, _ = add(f'{base}/slow/index.m3u8', filename='slow')
        wait(lambda: (task(t4['id']) or {}).get('downloaded_bytes', 0) > 200_000, 20)
        ui.invoke('pause_download_cmd', {'id': t4['id']})
        paused = wait(lambda: (task(t4['id']) or {}).get('status') == 'paused', 10)
        got = task(t4['id'])['downloaded_bytes']
        check('pause: a running stream pauses with its progress kept', bool(paused) and got > 0, task(t4['id']))
        DELAYS['/slow/'] = 0.05
        ui.invoke('resume_download_cmd', {'id': t4['id']})
        finished = wait(lambda: (task(t4['id']) or {}).get('status') == 'completed', 40)
        check('resume: finishes byte-exact after a pause', bool(finished) and same(task(t4['id']), b''.join(slow)), task(t4['id']) and task(t4['id'])['status'])

        # ---- killed mid-download, resumed on the next start
        DELAYS['/slow/'] = 0.4
        t5, _ = add(f'{base}/slow/index.m3u8', filename='killed')
        wait(lambda: (task(t5['id']) or {}).get('downloaded_bytes', 0) > 300_000, 20)
        time.sleep(2.5)  # let the progress record be written
        app.kill()
        time.sleep(1.5)
        DELAYS['/slow/'] = 0.02
        app = launch(env)
        ui = attach()
        resumed = wait(lambda: (ok(ui.invoke('get_download_cmd', {'id': t5['id']})) or {}).get('status') == 'completed', 60)
        t5n = ok(ui.invoke('get_download_cmd', {'id': t5['id']}))
        check('restart: the app resumes the stream by itself and finishes byte-exact', bool(resumed) and same(t5n, b''.join(slow)), t5n and t5n['status'])

        # ---- cancel removes partial files
        DELAYS['/slow/'] = 0.4
        t6, _ = add(f'{base}/slow/index.m3u8', filename='cancelled')
        wait(lambda: (ok(ui.invoke('get_download_cmd', {'id': t6['id']})) or {}).get('downloaded_bytes', 0) > 100_000, 20)
        ui.invoke('delete_download_cmd', {'id': t6['id'], 'deleteFile': True})
        time.sleep(1.5)
        left = [n for n in partials() if n.startswith('cancelled')]
        check('remove: partial files of a cancelled stream are deleted', left == [], left)
        DELAYS.clear()

        # ---- the browser bridge hands streams to the app (quality is chosen there)
        code, body = bridge('/download', {'url': f'{base}/show/master.m3u8', 'referer': 'https://example.com/watch'})
        check('bridge: a stream link is routed to the app instead of downloading blindly', code == 200 and body.get('stream') is True, (code, body))
        before = len(ok(ui.invoke('get_all_downloads_cmd')) or [])
        time.sleep(1)
        after = len(ok(ui.invoke('get_all_downloads_cmd')) or [])
        check('bridge: nothing was added behind the user\'s back', before == after, (before, after))
    finally:
        try:
            app.kill()
        except Exception:
            pass
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
