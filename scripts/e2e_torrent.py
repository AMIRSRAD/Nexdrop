"""End-to-end check of torrent support in the REAL app.

    cargo build --manifest-path src-tauri/Cargo.toml --example seed_torrent
    cargo build --manifest-path src-tauri/Cargo.toml
    python scripts/e2e_torrent.py

Starts a local swarm (tracker + seeder, see torrent_swarm.py), launches the
built app with a WebView2 debugging port, and calls the app's real Tauri
commands through DevTools, i.e. the same IPC the UI uses. It then checks the
results on disk and in the database.

It uses your real Nexdrop database, so the database is backed up first and
restored afterwards. Downloads go to a temporary folder. Close Nexdrop first.
"""
import base64, hashlib, json, os, shutil, socket, sqlite3, struct, subprocess, sys, tempfile, time, urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.path.join(ROOT, 'src-tauri', 'target', 'debug', 'nexdrop.exe')
SWARM = os.path.join(ROOT, 'scripts', 'torrent_swarm.py')
DATA = os.path.join(os.environ['LOCALAPPDATA'], 'Nexdrop')
DB = os.path.join(DATA, 'nexdrop.db')
BACKUP = os.path.join(tempfile.gettempdir(), 'nexdrop_torrent_e2e_backup')
DEBUG_PORT = 9223
BRIDGE = 'http://127.0.0.1:18888'

results = []


def check(name, ok, detail=''):
    results.append((name, ok))
    print(('PASS  ' if ok else 'FAIL  ') + name + (f'  [{detail}]' if detail and not ok else ''), flush=True)


# ------------------------------------------------------------------ minimal websocket client
class WS:
    def __init__(self, url):
        host, rest = url[5:].split('/', 1)
        h, p = host.split(':')
        self.s = socket.create_connection((h, int(p)), timeout=60)
        key = base64.b64encode(os.urandom(16)).decode()
        self.s.sendall((f'GET /{rest} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n'
                        f'Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n').encode())
        buf = b''
        while b'\r\n\r\n' not in buf:
            buf += self.s.recv(4096)
        assert b' 101 ' in buf.split(b'\r\n')[0], buf[:200]
        self.rest = buf.split(b'\r\n\r\n', 1)[1]
        self.n = 0

    def _read(self, n):
        while len(self.rest) < n:
            chunk = self.s.recv(65536)
            if not chunk:
                raise EOFError
            self.rest += chunk
        out, self.rest = self.rest[:n], self.rest[n:]
        return out

    def send(self, text):
        data = text.encode()
        head = bytearray([0x81])
        n = len(data)
        if n < 126:
            head.append(0x80 | n)
        elif n < 65536:
            head += bytes([0x80 | 126]) + struct.pack('>H', n)
        else:
            head += bytes([0x80 | 127]) + struct.pack('>Q', n)
        mask = os.urandom(4)
        head += mask
        self.s.sendall(bytes(head) + bytes(b ^ mask[i % 4] for i, b in enumerate(data)))

    def recv(self):
        msg = b''
        while True:
            b1, b2 = self._read(2)
            fin, op, n = b1 & 0x80, b1 & 0x0F, b2 & 0x7F
            if n == 126:
                n = struct.unpack('>H', self._read(2))[0]
            elif n == 127:
                n = struct.unpack('>Q', self._read(8))[0]
            payload = self._read(n)
            if op in (1, 0, 2):
                msg += payload
                if fin:
                    return msg.decode()
            elif op == 8:
                raise EOFError

    def call(self, method, params=None):
        self.n += 1
        mid = self.n
        self.send(json.dumps({'id': mid, 'method': method, 'params': params or {}}))
        while True:
            m = json.loads(self.recv())
            if m.get('id') == mid:
                return m


class App:
    def __init__(self, ws):
        self.ws = ws

    def invoke(self, cmd, args=None):
        expr = f"window.__TAURI_INTERNALS__.invoke({json.dumps(cmd)}, {json.dumps(args or {})}).then(v => ({{ok: v}}), e => ({{err: String(e)}}))"
        r = self.ws.call('Runtime.evaluate', {'expression': expr, 'awaitPromise': True, 'returnByValue': True})
        res = r.get('result', {}).get('result', {})
        if 'value' not in res:
            return {'err': json.dumps(r)[:300]}
        return res['value']


def http(path, body=None, origin='chrome-extension://e2e'):
    req = urllib.request.Request(BRIDGE + path, data=None if body is None else json.dumps(body).encode(), method='GET' if body is None else 'POST')
    req.add_header('Origin', origin)
    if body is not None:
        req.add_header('Content-Type', 'application/json')
    with urllib.request.urlopen(req, timeout=20) as r:
        return r.status, json.loads(r.read() or b'{}')


def blob(n, seed):
    return bytes((i * 31 + seed * 7 + 5) % 251 for i in range(n))


def main():
    try:
        urllib.request.urlopen(BRIDGE + '/status', timeout=1)
        print('A Nexdrop instance is already running on :18888; close it first.')
        return 2
    except Exception:
        pass

    os.makedirs(BACKUP, exist_ok=True)
    for ext in ('', '-wal', '-shm'):
        if os.path.exists(DB + ext):
            shutil.copy2(DB + ext, os.path.join(BACKUP, 'nexdrop.db' + ext))
    print('database backed up', flush=True)

    swarm = subprocess.Popen([sys.executable, SWARM], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    magnet = None
    for line in swarm.stdout:
        if line.startswith('MAGNET '):
            magnet = line.split(' ', 1)[1].strip()
            break
    if not magnet or magnet == 'None':
        print('swarm failed to start')
        swarm.kill()
        return 1
    print('swarm up:', magnet[:60], '...', flush=True)

    dest = tempfile.mkdtemp(prefix='nexdrop_tor_dest_')
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

        # ---- bridge routes magnet links to the UI instead of downloading blindly
        code, body = http('/download', {'url': magnet})
        check('bridge: magnet link is routed to the Add torrent dialog', code == 200 and body.get('torrent') is True, (code, body))

        # ---- probe (real metadata exchange with the seeder through the tracker)
        t0 = time.time()
        probe = ui.invoke('probe_torrent_cmd', {'url': magnet, 'bytes': None})
        info = probe.get('ok')
        check('probe: magnet resolves via tracker + peer', bool(info), probe)
        if not info:
            return 1
        print(f'      metadata fetched in {time.time() - t0:.1f}s', flush=True)
        check('probe: name and file list', info['name'] == 'Nexdrop Swarm Test' and len(info['files']) == 3 and info['total_bytes'] == 32_500_000, info)
        idx = {f['path'].replace('\\', '/').rsplit('/', 1)[-1]: f['index'] for f in info['files']}
        wanted = [idx['movie.bin'], idx['cover.bin']]

        # ---- bad input
        bad = ui.invoke('probe_torrent_cmd', {'url': 'ftp://example.com/x.torrent', 'bytes': None})
        check('probe: rejects things that are not magnet/.torrent links', 'err' in bad, bad)

        # ---- add with a file selection
        add = ui.invoke('add_torrent_cmd', {'req': {'info_hash': info['info_hash'], 'source': magnet, 'destination_path': dest, 'selected_files': wanted}})
        task = add.get('ok')
        check('add: torrent queued', bool(task) and task.get('kind') == 'torrent' and task['total_bytes'] == 24_500_000, add)
        dup = ui.invoke('add_torrent_cmd', {'req': {'info_hash': info['info_hash'], 'source': magnet, 'destination_path': dest, 'selected_files': wanted}})
        check('add: the same torrent twice is refused', 'already in your list' in json.dumps(dup), dup)
        none = ui.invoke('add_torrent_cmd', {'req': {'info_hash': info['info_hash'], 'destination_path': dest, 'selected_files': []}})
        check('add: no files selected is refused', 'err' in none, none)

        # ---- wait for completion
        done = None
        deadline = time.time() + 90
        while time.time() < deadline:
            all_tasks = ui.invoke('get_all_downloads_cmd').get('ok') or []
            done = next((t for t in all_tasks if t['id'] == task['id']), None)
            if done and done['status'] in ('completed', 'failed'):
                break
            time.sleep(1)
        check('download: completed', bool(done) and done['status'] == 'completed', done and (done['status'], done.get('error_message')))
        if not done or done['status'] != 'completed':
            return 1
        check('download: keeps sharing afterwards', done.get('seeding') is True, done.get('seeding'))

        folder = os.path.join(dest, 'Nexdrop Swarm Test')
        movie = os.path.join(folder, 'movie.bin')
        cover = os.path.join(folder, 'cover.bin')
        check('files: movie.bin is byte-exact', os.path.exists(movie) and open(movie, 'rb').read() == blob(24_000_000, 1))
        check('files: cover.bin is byte-exact', os.path.exists(cover) and open(cover, 'rb').read() == blob(500_000, 3))
        extra = os.path.join(folder, 'extras', 'commentary.bin')
        check('files: the unselected file was NOT downloaded', not os.path.exists(extra) or os.path.getsize(extra) < 8_000_000 or open(extra, 'rb').read() != blob(8_000_000, 2))
        try:
            zone = open(movie + ':Zone.Identifier', encoding='utf-8', errors='replace').read()
        except OSError:
            zone = ''
        check('files: Mark of the Web on downloaded files', 'ZoneId=3' in zone, zone)

        con = sqlite3.connect(DB, timeout=10)
        row = con.execute('SELECT kind, info_hash, seeding, length(torrent_bytes), torrent_started FROM downloads WHERE id=?', (task['id'],)).fetchone()
        con.close()
        check('db: kind, hash, metadata and state stored', row and row[0] == 'torrent' and row[1] == info['info_hash'] and (row[3] or 0) > 100 and row[4] == 1, row)

        # ---- sharing controls
        time.sleep(6)  # let the seeding monitor run once
        stop = ui.invoke('stop_seeding_cmd', {'id': task['id']})
        after = next(t for t in ui.invoke('get_all_downloads_cmd')['ok'] if t['id'] == task['id'])
        check('sharing: stop sharing', 'err' not in stop and after.get('seeding') is False, (stop, after.get('seeding')))
        start = ui.invoke('start_seeding_cmd', {'id': task['id']})
        after = next(t for t in ui.invoke('get_all_downloads_cmd')['ok'] if t['id'] == task['id'])
        check('sharing: start sharing again', 'err' not in start and after.get('seeding') is True, (start, after.get('seeding')))

        # ---- removal
        rm = ui.invoke('delete_download_cmd', {'id': task['id'], 'deleteFile': True})
        gone = not any(t['id'] == task['id'] for t in ui.invoke('get_all_downloads_cmd')['ok'])
        check('remove: deleted from the list', 'err' not in rm and gone, rm)
        check('remove: downloaded files were deleted', not os.path.exists(movie) and not os.path.exists(cover), os.listdir(folder) if os.path.isdir(folder) else 'folder gone')
    finally:
        app.terminate()
        try:
            app.wait(timeout=10)
        except Exception:
            app.kill()
        swarm.terminate()
        subprocess.run(['taskkill', '/F', '/IM', 'seed_torrent.exe'], capture_output=True)
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
        print('database restored, temporary files removed', flush=True)

    failed = [n for n, ok in results if not ok]
    print(f'\n{len(results) - len(failed)}/{len(results)} checks passed')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
