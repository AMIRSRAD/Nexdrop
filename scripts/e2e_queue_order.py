"""Queue order controls in the REAL app: download next, move up/down/end, download now past a
full queue, order surviving a restart, two queues running side by side.

    python scripts/e2e_queue_order.py

Uses your real Nexdrop database (backed up first, restored afterwards). Close Nexdrop first.
"""
import http.server, json, os, shutil, socketserver, subprocess, sys, tempfile, threading, time, urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'scripts'))
from e2e_torrent import WS, App

EXE = os.path.join(ROOT, 'src-tauri', 'target', 'release', 'nexdrop.exe')
DB = os.path.join(os.environ['LOCALAPPDATA'], 'Nexdrop', 'nexdrop.db')
BACKUP = os.path.join(tempfile.gettempdir(), 'nexdrop_order_e2e_backup')
DEBUG_PORT = 9232
FILES = {}
results = []
STARTS = []  # order in which the server first saw each file requested


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
        name = self.path.split('?')[0].lstrip('/')
        data = FILES.get(name)
        if data is None:
            self.send_response(404); self.send_header('Content-Length', '0'); self.end_headers(); return
        start, end = 0, len(data) - 1
        rng = self.headers.get('Range')
        if rng and rng.startswith('bytes=') and not head:
            a, _, b = rng[6:].partition('-')
            start = int(a or 0); end = min(int(b), len(data) - 1) if b else len(data) - 1
            self.send_response(206); self.send_header('Content-Range', f'bytes {start}-{end}/{len(data)}')
        else:
            self.send_response(200)
        self.send_header('Content-Length', str(end - start + 1)); self.send_header('Accept-Ranges', 'bytes'); self.send_header('Content-Type', 'video/mp4')
        self.end_headers()
        if head:
            return
        if name not in STARTS and start == 0:
            STARTS.append(name)
        try:
            sent = start
            while sent <= end:
                n = min(32 * 1024, end - sent + 1)
                self.wfile.write(data[sent:sent + n]); sent += n
                time.sleep(0.12)  # slow: a download takes a few seconds
        except Exception:
            pass

    def do_HEAD(self):
        self._go(True)

    def do_GET(self):
        self._go(False)


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


def launch(env):
    app = subprocess.Popen([EXE], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
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
    return app, App(ws)


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
    for i in range(5):
        FILES[f'f{i}.mp4'] = blob(250_000, i)
    for i in range(5, 9):
        FILES[f'f{i}.mp4'] = blob(2_000_000, i)  # slow enough to still be running when checked
    server = Server(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_address[1]}'
    dest = tempfile.mkdtemp(prefix='nexdrop_order_dest_')
    env = dict(os.environ, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=f'--remote-debugging-port={DEBUG_PORT}')
    app = None
    try:
        app, ui = launch(env)
        ok = lambda r: r.get('ok')
        task = lambda i: ok(ui.invoke('get_download_cmd', {'id': i})) or {}
        line = lambda ids: [i for i in sorted(ids, key=lambda i: (task(i).get('queue_order'), task(i).get('created_at')))]

        # a queue that runs one download at a time, closed so everything waits
        q = ok(ui.invoke('save_queue_cmd', {'queue': {'id': '', 'name': 'Line', 'schedule_enabled': False, 'days': ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'],
                                                      'start_time': '02:00', 'stop_time': None, 'max_active': 1, 'speed_limit': 0, 'after_done': 'none',
                                                      'after_done_program': None, 'retry_count': 0, 'retry_delay_secs': 60, 'position': 0}}))
        qid = q['id']
        ids = []
        for i in range(5):
            r = ui.invoke('add_download_cmd', {'req': {'url': f'{base}/f{i}.mp4', 'destination_path': dest, 'connections': 1, 'queue_id': qid, 'hold': True}})
            ids.append((ok(r) or {}).get('id'))
            time.sleep(1.1)  # distinct added times
        check('order: five downloads wait in the queue', all(task(i).get('status') == 'paused' for i in ids), [task(i).get('status') for i in ids])
        check('order: they wait in the order added', line(ids) == ids, line(ids))

        ui.invoke('reorder_downloads_cmd', {'ids': [ids[3]], 'action': 'next'})
        check('order: "download next" puts one first', line(ids) == [ids[3], ids[0], ids[1], ids[2], ids[4]], [ids.index(x) for x in line(ids)])
        ui.invoke('reorder_downloads_cmd', {'ids': [ids[3]], 'action': 'down'})
        check('order: move down by one', line(ids) == [ids[0], ids[3], ids[1], ids[2], ids[4]], [ids.index(x) for x in line(ids)])
        ui.invoke('reorder_downloads_cmd', {'ids': [ids[0]], 'action': 'bottom'})
        check('order: move to the end', line(ids) == [ids[3], ids[1], ids[2], ids[4], ids[0]], [ids.index(x) for x in line(ids)])
        ui.invoke('reorder_downloads_cmd', {'ids': [ids[4]], 'action': 'up'})
        expected = [ids[3], ids[1], ids[2], ids[4], ids[0]]  # 4 was already above 0: now one step up
        expected = [ids[3], ids[1], ids[4], ids[2], ids[0]]
        check('order: move up by one', line(ids) == expected, [ids.index(x) for x in line(ids)])

        # the queue starts: downloads run one at a time, in this order
        STARTS.clear()
        ui.invoke('start_queue_cmd', {'id': qid})
        done = wait(lambda: all(task(i).get('status') == 'completed' for i in ids), 90)
        names = [f'f{ids.index(x)}.mp4' for x in expected]
        check('run: everything finishes', bool(done), [task(i).get('status') for i in ids])
        check('run: the server saw them start in the queue order', STARTS == names, STARTS)

        # download now: ignores a full queue
        r2 = [(ok(ui.invoke('add_download_cmd', {'req': {'url': f'{base}/f{i}.mp4', 'destination_path': dest + '_b', 'connections': 1, 'queue_id': qid, 'hold': True}})) or {}).get('id') for i in (6, 7, 8)]
        slow = ok(ui.invoke('add_download_cmd', {'req': {'url': f'{base}/f5.mp4', 'destination_path': dest + '_c', 'connections': 1, 'queue_id': qid}}))['id']
        wait(lambda: task(slow).get('status') == 'downloading', 15)
        for i in r2:
            ui.invoke('resume_download_cmd', {'id': i})
        time.sleep(1)
        waiting = task(r2[0]).get('status') in ('queued', 'paused')
        ui.invoke('start_download_now_cmd', {'id': r2[0]})
        time.sleep(1.5)
        both = task(r2[0]).get('status') == 'downloading' and task(slow).get('status') == 'downloading'
        check('now: a full queue still lets "Download now" start one beside it', waiting and both, (task(r2[0]).get('status'), task(slow).get('status')))
        check('now: and it goes first in line', line(r2)[0] == r2[0], [r2.index(x) for x in line(r2)])
        wait(lambda: all(task(i).get('status') == 'completed' for i in r2 + [slow]), 60)

        # order survives a restart
        for i in ids:
            ui.invoke('cancel_download_cmd', {'id': i, 'delete_file': False})
        keep = [(ok(ui.invoke('add_download_cmd', {'req': {'url': f'{base}/f{i}.mp4', 'destination_path': dest + '_d', 'connections': 1, 'queue_id': qid, 'hold': True}})) or {}).get('id') for i in (0, 1, 2)]
        ui.invoke('reorder_downloads_cmd', {'ids': [keep[2]], 'action': 'next'})
        before = line(keep)
        app.kill(); time.sleep(1.5)
        app, ui = launch(env)
        task = lambda i: ok(ui.invoke('get_download_cmd', {'id': i})) or {}
        check('restart: the order is kept', line(keep) == before == [keep[2], keep[0], keep[1]], [keep.index(x) for x in line(keep)])
    finally:
        if app:
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
        for d in (dest, dest + '_b', dest + '_c', dest + '_d'):
            shutil.rmtree(d, ignore_errors=True)
        print('database restored', flush=True)

    failed = [n for n, passed in results if not passed]
    print(f'\n{len(results) - len(failed)}/{len(results)} checks passed')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
