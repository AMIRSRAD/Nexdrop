"""End-to-end check of the real Nexdrop app through its browser bridge.

    pnpm build && cargo build --release --manifest-path src-tauri/Cargo.toml
    python scripts/e2e_bridge.py

Starts a local HTTP server, runs the built app (release build), queues downloads
over 127.0.0.1:18888 exactly as the browser extension would, and inspects the
results: Mark of the Web, expected-checksum verification, saved logins, queue
advancement and the bridge's origin rules.

It uses your real Nexdrop database and Downloads folder, so the database is
backed up first and restored at the end, and every file it creates is removed.
Close Nexdrop before running (it refuses to run if the bridge port is busy).
"""
import base64, hashlib, http.server, json, os, shutil, socketserver, sqlite3, subprocess, sys, tempfile, threading, time, urllib.error, urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.path.join(ROOT, 'src-tauri', 'target', 'release', 'nexdrop.exe')
DATA = os.path.join(os.environ['LOCALAPPDATA'], 'Nexdrop')
DB = os.path.join(DATA, 'nexdrop.db')
BACKUP = os.path.join(tempfile.gettempdir(), 'nexdrop_e2e_db_backup')
BRIDGE = 'http://127.0.0.1:18888'
PORT = 8765
AUTH = 'Basic ' + base64.b64encode(b'e2euser:e2epass').decode()

FILES = {
    'nexdrop_e2e_crlf.bin': bytes((i * 3 + 7) % 251 for i in range(100_000)),
    'nexdrop_e2e_cookie.bin': bytes((i * 13 + 1) % 251 for i in range(300_000)),
    'nexdrop_e2e_ok.bin': bytes((i * 7 + 3) % 251 for i in range(2_000_000)),
    'nexdrop_e2e_bad.bin': bytes((i * 5 + 1) % 251 for i in range(1_000_000)),
    'nexdrop_e2e_auth.bin': bytes((i * 11 + 9) % 251 for i in range(500_000)),
}
for i in range(6):
    FILES[f'nexdrop_e2e_q{i}.bin'] = bytes((i + j) % 251 for j in range(300_000))

seen_auth = []
seen_cookies = []
results = []


def check(name, ok, detail=''):
    results.append((name, ok))
    print(('PASS  ' if ok else 'FAIL  ') + name + (f'  [{detail}]' if detail and not ok else ''))


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def _serve(self, head):
        name = self.path.lstrip('/')
        data = FILES.get(name)
        if data is None:
            self.send_response(404); self.end_headers(); return
        if 'cookie' in name:
            seen_cookies.append(self.headers.get('Cookie'))
        if 'auth' in name:
            seen_auth.append(self.headers.get('Authorization'))
            if self.headers.get('Authorization') != AUTH:
                self.send_response(401); self.send_header('WWW-Authenticate', 'Basic'); self.end_headers(); return
        if '_q' in name and not head:
            time.sleep(1.2)  # keep downloads busy so the queue limit matters
        self.send_response(200)
        self.send_header('Content-Length', str(len(data)))
        self.send_header('Content-Type', 'application/octet-stream')
        self.end_headers()
        if not head:
            self.wfile.write(data)

    def do_HEAD(self):
        self._serve(True)

    def do_GET(self):
        self._serve(False)


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


def bridge(path, body=None, origin='chrome-extension://e2etest', content_type='application/json'):
    req = urllib.request.Request(BRIDGE + path, data=None if body is None else json.dumps(body).encode(), method='GET' if body is None else 'POST')
    if origin:
        req.add_header('Origin', origin)
    if body is not None:
        req.add_header('Content-Type', content_type)
    try:
        with urllib.request.urlopen(req, timeout=20) as r:
            return r.status, json.loads(r.read() or b'{}')
    except urllib.error.HTTPError as e:
        try:
            return e.code, json.loads(e.read() or b'{}')
        except Exception:
            return e.code, {}


def rows():
    con = sqlite3.connect(DB, timeout=10)
    con.row_factory = sqlite3.Row
    try:
        return [dict(r) for r in con.execute("SELECT * FROM downloads WHERE filename LIKE 'nexdrop_e2e%' OR url LIKE 'http://127.0.0.1:8765/%'")]
    finally:
        con.close()


def default_dest():
    con = sqlite3.connect(DB, timeout=10)
    try:
        r = con.execute("SELECT value FROM settings WHERE key='app_settings'").fetchone()
        if r:
            return json.loads(r[0]).get('default_destination')
    finally:
        con.close()
    return os.path.join(os.environ['USERPROFILE'], 'Downloads')


def main():
    # Refuse to run if a Nexdrop instance already owns the bridge port.
    try:
        urllib.request.urlopen(BRIDGE + '/status', timeout=1)
        print('A Nexdrop instance is already running on :18888; aborting so we do not touch it.')
        return 2
    except Exception:
        pass

    os.makedirs(BACKUP, exist_ok=True)
    for ext in ('', '-wal', '-shm'):
        if os.path.exists(DB + ext):
            shutil.copy2(DB + ext, os.path.join(BACKUP, 'nexdrop.db' + ext))
    print('database backed up')

    server = Server(('127.0.0.1', PORT), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()

    # A pre-existing plaintext login, as an older version would have stored it.
    con = sqlite3.connect(DB, timeout=10)
    con.execute("INSERT OR REPLACE INTO site_credentials (id, domain, username, password, token, notes) VALUES ('e2e-cred','127.0.0.1','e2euser','e2epass',NULL,'e2e')")
    con.commit(); con.close()

    dest = default_dest()
    proc = subprocess.Popen([EXE], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    created = []
    try:
        for _ in range(60):
            try:
                urllib.request.urlopen(BRIDGE + '/status', timeout=1); break
            except Exception:
                time.sleep(0.5)
        else:
            print('app did not start'); return 1
        time.sleep(1.0)
        print(f'app running, destination = {dest}')

        # --- like IDM: with the default setting a browser link opens the download window first
        code, body = bridge('/download', {'url': f'http://127.0.0.1:{PORT}/nexdrop_e2e_cookie.bin', 'cookies': 'sid=abc123; theme=dark'})
        time.sleep(1.5)
        check('browser link: the download window is shown instead of starting blindly', code == 200 and body.get('prompt') is True and rows() == [], (code, body, len(rows())))
        # now switch to "start immediately" and restart, as a user would
        proc.terminate()
        try:
            proc.wait(timeout=10)
        except Exception:
            proc.kill()
        time.sleep(1.5)
        con = sqlite3.connect(DB, timeout=10)
        r = con.execute("SELECT value FROM settings WHERE key='app_settings'").fetchone()
        cfg = json.loads(r[0]) if r else {}
        cfg['browser_prompt'] = False
        con.execute("INSERT OR REPLACE INTO settings (key, value) VALUES ('app_settings', ?)", (json.dumps(cfg),))
        con.commit(); con.close()
        proc = subprocess.Popen([EXE], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(60):
            try:
                urllib.request.urlopen(BRIDGE + '/status', timeout=1); break
            except Exception:
                time.sleep(0.5)
        time.sleep(1.0)

        # --- bridge hardening
        code, _ = bridge('/download', {'url': f'http://127.0.0.1:{PORT}/nexdrop_e2e_ok.bin'}, origin='https://evil.example')
        check('bridge rejects a web page origin', code == 403, code)
        code, _ = bridge('/download', {'url': f'http://127.0.0.1:{PORT}/nexdrop_e2e_ok.bin'}, origin=None)
        check('bridge rejects a request with no extension origin', code == 403, code)
        code, body = bridge('/download', {'url': f'http://127.0.0.1:{PORT}/nexdrop_e2e_ok.bin', 'expected_checksum': 'abc'})
        check('invalid expected checksum is rejected up front', code == 400 and 'characters' in json.dumps(body), (code, body))

        # --- stored login got encrypted at startup
        con = sqlite3.connect(DB, timeout=10)
        pw = con.execute("SELECT password FROM site_credentials WHERE id='e2e-cred'").fetchone()[0]
        con.close()
        check('plaintext login password was encrypted at startup', pw.startswith('dpapi1:') and 'e2epass' not in pw, pw[:20])

        # --- downloads
        ok_hash = hashlib.sha256(FILES['nexdrop_e2e_ok.bin']).hexdigest()
        code, _ = bridge('/download', {'url': f'http://127.0.0.1:{PORT}/nexdrop_e2e_ok.bin', 'expected_checksum': 'SHA256:' + ok_hash.upper()})
        check('queue download with correct expected hash', code == 200, code)
        code, _ = bridge('/download', {'url': f'http://127.0.0.1:{PORT}/nexdrop_e2e_bad.bin', 'expected_checksum': '0' * 64})
        check('queue download with WRONG expected hash', code == 200, code)
        code, _ = bridge('/download', {'url': f'http://127.0.0.1:{PORT}/nexdrop_e2e_auth.bin'})
        check('queue download that needs a saved login', code == 200, code)
        code, body = bridge('/download', {'url': f'http://127.0.0.1:{PORT}/nexdrop_e2e_cookie.bin', 'cookies': 'sid=abc123; theme=dark'})
        check('browser link with cookies starts immediately when the window is turned off', code == 200 and body.get('task_id'), (code, body))
        code, _ = bridge('/download', {'url': f'http://127.0.0.1:{PORT}/nexdrop_e2e_crlf.bin', 'cookies': 'a=1\r\nInjected: yes'})
        check('a cookie value with line breaks is dropped, not forwarded', code == 200, code)
        for i in range(6):
            bridge('/download', {'url': f'http://127.0.0.1:{PORT}/nexdrop_e2e_q{i}.bin'})

        deadline = time.time() + 60
        while time.time() < deadline:
            rs = rows()
            if len(rs) >= 11 and all(r['status'] in ('completed', 'failed') for r in rs):
                break
            time.sleep(0.5)
        rs = {r['url'].rsplit('/', 1)[1]: r for r in rows()}
        for r in rs.values():
            created.append((r['destination_path'], r['filename']))

        ok = rs.get('nexdrop_e2e_ok.bin', {})
        check('correct hash: completed', ok.get('status') == 'completed', ok.get('status'))
        path = os.path.join(ok.get('destination_path', ''), ok.get('filename', ''))
        check('correct hash: file bytes are exact', os.path.exists(path) and open(path, 'rb').read() == FILES['nexdrop_e2e_ok.bin'])
        check('correct hash: sha256 recorded', (ok.get('checksum_sha256') or '') == ok_hash)
        try:
            zone = open(path + ':Zone.Identifier', encoding='utf-8', errors='replace').read()
        except OSError as e:
            zone = f'ERR {e}'
        check('Mark of the Web written (ZoneId=3 + HostUrl)', 'ZoneId=3' in zone and f'HostUrl=http://127.0.0.1:{PORT}/nexdrop_e2e_ok.bin' in zone, zone)

        bad = rs.get('nexdrop_e2e_bad.bin', {})
        check('wrong hash: marked failed', bad.get('status') == 'failed', bad.get('status'))
        check('wrong hash: message explains the integrity failure', 'Integrity check failed' in (bad.get('error_message') or ''), bad.get('error_message'))
        bad_path = os.path.join(bad.get('destination_path', ''), bad.get('filename', ''))
        check('wrong hash: corrupt file was removed', not os.path.exists(bad_path), bad_path)

        au = rs.get('nexdrop_e2e_auth.bin', {})
        check('saved login: completed', au.get('status') == 'completed', (au.get('status'), au.get('error_message')))
        check('saved login: server saw the Basic Authorization header', AUTH in seen_auth, seen_auth[:3])
        ap = os.path.join(au.get('destination_path', ''), au.get('filename', ''))
        check('saved login: file bytes are exact', os.path.exists(ap) and open(ap, 'rb').read() == FILES['nexdrop_e2e_auth.bin'])

        ck = rs.get('nexdrop_e2e_cookie.bin', {})
        cp = os.path.join(ck.get('destination_path', ''), ck.get('filename', ''))
        check('cookies: the server received the browser Cookie header', 'sid=abc123; theme=dark' in seen_cookies, seen_cookies)
        check('cookies: file bytes are exact', os.path.exists(cp) and open(cp, 'rb').read() == FILES['nexdrop_e2e_cookie.bin'])
        q = [rs.get(f'nexdrop_e2e_q{i}.bin', {}) for i in range(6)]
        check('queue advances: all 6 downloads (limit 3 at a time) completed', all(r.get('status') == 'completed' for r in q), [r.get('status') for r in q])
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=10)
        except Exception:
            proc.kill()
        time.sleep(1)
        server.shutdown()
        # cleanup: downloaded files, then restore the database exactly as it was
        for d, f in created:
            for suffix in ('', '.nexpart', '.nexpart.map'):
                try:
                    os.remove(os.path.join(d, f + suffix))
                except OSError:
                    pass
        for ext in ('', '-wal', '-shm'):
            try:
                os.remove(DB + ext)
            except OSError:
                pass
            b = os.path.join(BACKUP, 'nexdrop.db' + ext)
            if os.path.exists(b):
                shutil.copy2(b, DB + ext)
        print('database restored, test files removed')

    failed = [n for n, ok in results if not ok]
    print(f'\n{len(results) - len(failed)}/{len(results)} checks passed')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
