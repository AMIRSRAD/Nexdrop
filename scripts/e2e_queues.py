"""End-to-end check of download queues in the REAL app.

    pnpm build && cargo build --release --manifest-path src-tauri/Cargo.toml
    python scripts/e2e_queues.py

Starts a local HTTP server (ranges, throttling, one flaky file), launches the
built app with a WebView2 debugging port and drives the real Tauri commands
the UI uses: creating queues, schedules, Start now / Stop, per-queue limits,
"when finished" actions, automatic retry, moving and deleting queues.

It uses your real Nexdrop database, so the database is backed up first and
restored afterwards. Downloads go to a temporary folder. Close Nexdrop first.
"""
import datetime, hashlib, http.server, json, os, shutil, socketserver, subprocess, sys, tempfile, threading, time, urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'scripts'))
from e2e_torrent import WS, App  # tiny CDP client

EXE = os.path.join(ROOT, 'src-tauri', 'target', 'release', 'nexdrop.exe')
DATA = os.path.join(os.environ['LOCALAPPDATA'], 'Nexdrop')
DB = os.path.join(DATA, 'nexdrop.db')
BACKUP = os.path.join(tempfile.gettempdir(), 'nexdrop_queues_e2e_backup')
DEBUG_PORT = 9226
BRIDGE = 'http://127.0.0.1:18888'
results = []


def check(name, ok, detail=''):
    results.append((name, ok))
    print(('PASS  ' if ok else 'FAIL  ') + name + (f'  [{detail}]' if detail and not ok else ''), flush=True)


def blob(n, seed):
    return bytes((i * 31 + seed * 7 + 5) % 251 for i in range(n))


FILES = {}          # name -> bytes
FLAKY_SEEN = set()  # paths whose first GET already failed
LOCK = threading.Lock()
STATS = {'peak_parallel': 0, 'now': 0}


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *a):
        pass

    def _data(self):
        path = self.path.split('?')[0].lstrip('/')
        return path, FILES.get(path)

    def do_HEAD(self):
        path, data = self._data()
        if data is None:
            self.send_response(404); self.send_header('Content-Length', '0'); self.end_headers(); return
        self.send_response(200)
        self.send_header('Content-Length', str(len(data)))
        self.send_header('Accept-Ranges', 'bytes')
        self.end_headers()

    def do_GET(self):
        path, data = self._data()
        if data is None:
            self.send_response(404); self.send_header('Content-Length', '0'); self.end_headers(); return
        if path.startswith('flaky'):
            with LOCK:
                first = path not in FLAKY_SEEN
                FLAKY_SEEN.add(path)
            if first:
                self.send_response(404); self.send_header('Content-Length', '0'); self.end_headers(); return
        start, end = 0, len(data) - 1
        rng = self.headers.get('Range')
        if rng and rng.startswith('bytes='):
            a, _, b = rng[6:].partition('-')
            start = int(a or 0)
            end = min(int(b), len(data) - 1) if b else len(data) - 1
            self.send_response(206)
            self.send_header('Content-Range', f'bytes {start}-{end}/{len(data)}')
        else:
            self.send_response(200)
        self.send_header('Content-Length', str(end - start + 1))
        self.end_headers()
        bps = int(path.split('_bps')[1].split('.')[0]) if '_bps' in path else 0
        with LOCK:
            STATS['now'] += 1
            STATS['peak_parallel'] = max(STATS['peak_parallel'], STATS['now'])
        try:
            sent, t0, piece = start, time.time(), 64 * 1024
            while sent <= end:
                n = min(piece, end - sent + 1)
                self.wfile.write(data[sent:sent + n])
                sent += n
                if bps:
                    due = (sent - start) / bps
                    el = time.time() - t0
                    if due > el:
                        time.sleep(due - el)
        except Exception:
            pass
        finally:
            with LOCK:
                STATS['now'] -= 1


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


def hhmm(dt):
    return dt.strftime('%H:%M')


def wait(cond, timeout=30, step=0.3):
    end = time.time() + timeout
    while time.time() < end:
        v = cond()
        if v:
            return v
        time.sleep(step)
    return cond()


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

    MB = 1024 * 1024
    FILES['small.bin'] = blob(300_000, 1)
    FILES['fast1.bin'] = blob(2 * MB, 2)
    FILES['limited.bin'] = blob(1 * MB, 3)
    for i in range(3):
        FILES[f'slow{i}_bps400000.bin'] = blob(1_200_000, 10 + i)
    FILES['steady_bps150000.bin'] = blob(6 * MB, 20)
    FILES['flaky_retry.bin'] = blob(400_000, 30)
    server = Server(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_address[1]}'

    dest = tempfile.mkdtemp(prefix='nexdrop_queue_dest_')
    marker = os.path.join(dest, 'after_done_ran.txt')
    bat = os.path.join(dest, 'after_done.bat')
    open(bat, 'w').write(f'@echo off\r\necho ran> "{marker}"\r\n')

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

        def ok(r):
            return r.get('ok')

        def queues():
            return ok(ui.invoke('get_queues_cmd')) or []

        def q(id_):
            return next((x for x in queues() if x['id'] == id_), None)

        def add(name, queue_id=None, **extra):
            req = {'url': f'{base}/{name}', 'destination_path': dest, 'connections': 2, **extra}
            if queue_id:
                req['queue_id'] = queue_id
            r = ui.invoke('add_download_cmd', {'req': req})
            return r.get('ok'), r

        def task(id_):
            return ok(ui.invoke('get_download_cmd', {'id': id_}))

        def save(**kw):
            base_q = {'id': '', 'name': 'Test', 'schedule_enabled': False, 'days': ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'],
                      'start_time': '02:00', 'stop_time': None, 'max_active': 0, 'speed_limit': 0, 'after_done': 'none',
                      'after_done_program': None, 'retry_count': 0, 'retry_delay_secs': 60, 'position': 0}
            r = ui.invoke('save_queue_cmd', {'queue': {**base_q, **kw}})
            return r.get('ok'), r

        def same(name, t):
            p = os.path.join(t['destination_path'], t['filename'])
            return os.path.exists(p) and open(p, 'rb').read() == FILES[name]

        now = datetime.datetime.now()
        all_days = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun']

        # ---- the Main queue exists (migrated from the old settings)
        main = q('main')
        check('main queue exists from the start', bool(main) and main['open'] is True, queues())

        # ---- validation
        bad, r = save(name='  ')
        check('queue: an empty name is refused', bad is None and 'err' in r, r)
        bad, r = save(name='x', schedule_enabled=True, days=[])
        check('queue: a schedule with no days is refused', bad is None, r)
        bad, r = save(name='x', after_done='program')
        check('queue: "run a program" without a program is refused', bad is None, r)
        _, r = ui.invoke('delete_queue_cmd', {'id': 'main'}), None
        check('queue: the main queue cannot be deleted', 'err' in ui.invoke('delete_queue_cmd', {'id': 'main'}))

        # ---- a queue outside its schedule keeps downloads waiting; Start now runs it
        later = now + datetime.timedelta(hours=3)
        night, r = save(name='Night', schedule_enabled=True, start_time=hhmm(later), stop_time=hhmm(later + datetime.timedelta(hours=1)))
        check('queue: created', bool(night) and bool(night['id']), r)
        info = q(night['id'])
        check('queue: closed outside its window, with a next start time', info['open'] is False and bool(info['next_start']), info)

        t1, r = add('fast1.bin', night['id'])
        check('queue: download added to the queue', bool(t1) and t1['queue_id'] == night['id'], r)
        time.sleep(4)
        t = task(t1['id'])
        check('queue: download waits while the queue is closed', t['status'] == 'queued' and t['downloaded_bytes'] == 0, t and (t['status'], t['downloaded_bytes']))
        check('queue: waiting count shows in the queue info', q(night['id'])['queued'] == 1, q(night['id']))

        ui.invoke('start_queue_cmd', {'id': night['id']})
        done = wait(lambda: (task(t1['id']) or {}).get('status') == 'completed', 30)
        t = task(t1['id'])
        check('queue: Start now runs it regardless of the schedule', bool(done) and same('fast1.bin', t), t and t['status'])
        check('queue: Start now ends when the queue is empty', wait(lambda: q(night['id'])['open'] is False, 10) is True, q(night['id']))

        # ---- schedule window open now: starts by itself; editing it closed pauses the running download
        win, _ = save(name='Window', schedule_enabled=True, start_time=hhmm(now - datetime.timedelta(hours=1)), stop_time=hhmm(now + datetime.timedelta(hours=1)))
        check('window: open now', q(win['id'])['open'] is True)
        t2, _ = add('steady_bps150000.bin', win['id'])
        started = wait(lambda: (task(t2['id']) or {}).get('status') == 'downloading' and (task(t2['id']) or {}).get('downloaded_bytes', 0) > 0, 20)
        check('window: download starts by itself', bool(started), task(t2['id']))
        # close the window by moving it into the future
        save(id=win['id'], name='Window', schedule_enabled=True, start_time=hhmm(later), stop_time=hhmm(later + datetime.timedelta(hours=1)))
        paused = wait(lambda: (task(t2['id']) or {}).get('status') == 'paused', 20)
        check('window: closing it pauses the running download', bool(paused), task(t2['id']))
        got_before = task(t2['id'])['downloaded_bytes']
        # reopen
        save(id=win['id'], name='Window', schedule_enabled=True, start_time=hhmm(now - datetime.timedelta(hours=1)), stop_time=hhmm(now + datetime.timedelta(hours=1)))
        resumed = wait(lambda: (task(t2['id']) or {}).get('status') == 'downloading', 20)
        check('window: reopening resumes it', bool(resumed), task(t2['id']))
        ui.invoke('stop_queue_cmd', {'id': win['id']})
        check('queue: Stop pauses what is running', wait(lambda: (task(t2['id']) or {}).get('status') == 'paused', 15) is True, task(t2['id']))
        ui.invoke('delete_download_cmd', {'id': t2['id'], 'deleteFile': True})

        # ---- start-only schedule: triggers by itself at its start time
        fresh = datetime.datetime.now()
        if fresh.second > 50:  # keep the start minute comfortably in the future
            time.sleep(12)
            fresh = datetime.datetime.now()
        trig, _ = save(name='Trigger', schedule_enabled=True, start_time=hhmm(fresh + datetime.timedelta(minutes=1)), stop_time=None)
        t3, _ = add('small.bin', trig['id'])
        time.sleep(3)
        check('trigger: waits before its start time', (task(t3['id']) or {}).get('status') == 'queued', task(t3['id']))
        target = (fresh.replace(second=0, microsecond=0) + datetime.timedelta(minutes=1))
        wait_s = max(0, (target - datetime.datetime.now()).total_seconds()) + 12
        print(f'      waiting {wait_s:.0f}s for the start time...', flush=True)
        trig_done = wait(lambda: (task(t3['id']) or {}).get('status') == 'completed', wait_s + 10, 1)
        check('trigger: starts by itself at its start time and finishes', bool(trig_done) and same('small.bin', task(t3['id'])), task(t3['id']))

        # ---- simultaneous downloads per queue
        lim, _ = save(name='One at a time', max_active=1)
        ids = [add(f'slow{i}_bps400000.bin', lim['id'])[0]['id'] for i in range(3)]
        peak = 0
        end = time.time() + 40
        while time.time() < end:
            ts = [task(i) for i in ids]
            peak = max(peak, sum(1 for x in ts if x['status'] == 'downloading'))
            if all(x['status'] == 'completed' for x in ts):
                break
            time.sleep(0.2)
        ts = [task(i) for i in ids]
        check('limits: at most one download runs at a time', peak == 1, peak)
        check('limits: all of them still finish, byte-exact', all(x['status'] == 'completed' for x in ts) and all(same(f'slow{i}_bps400000.bin', ts[i]) for i in range(3)), [x['status'] for x in ts])

        # ---- queue speed limit is shared and enforced
        slow, _ = save(name='Capped', speed_limit=256 * 1024)
        t0 = time.time()
        t4, _ = add('limited.bin', slow['id'])
        wait(lambda: (task(t4['id']) or {}).get('status') == 'completed', 30)
        secs = time.time() - t0
        check('limits: the queue speed cap is enforced (1 MiB at 256 KiB/s)', 3.0 <= secs <= 12 and same('limited.bin', task(t4['id'])), f'{secs:.1f}s')

        # ---- "when finished" runs a program
        prog, _ = save(name='Program', after_done='program', after_done_program=bat)
        t5, _ = add('small.bin', prog['id'], filename='prog_small.bin')
        wait(lambda: (task(t5['id']) or {}).get('status') == 'completed', 20)
        check('finish: the queue runs its program when empty', bool(wait(lambda: os.path.exists(marker), 15)), os.listdir(dest))

        # ---- automatic retry of a failed download
        retry, _ = save(name='Retrying', retry_count=2, retry_delay_secs=5)
        t6, _ = add('flaky_retry.bin', retry['id'])
        failed = wait(lambda: (task(t6['id']) or {}).get('status') == 'failed', 20)
        check('retry: a failing download is marked failed first', bool(failed), task(t6['id']))
        healed = wait(lambda: (task(t6['id']) or {}).get('status') == 'completed', 30)
        check('retry: it is retried automatically and completes', bool(healed) and same('flaky_retry.bin', task(t6['id'])), task(t6['id']))

        # ---- move between queues, delete moves downloads to Main
        t7, _ = add('small.bin', None, filename='mover.bin')
        wait(lambda: (task(t7['id']) or {}).get('status') == 'completed', 20)
        dq, _ = save(name='Temp')
        paused_t, _ = add('steady_bps150000.bin', dq['id'], filename='held.bin')
        ui.invoke('pause_download_cmd', {'id': paused_t['id']})
        mv = ui.invoke('move_to_queue_cmd', {'ids': [paused_t['id']], 'queueId': 'main'})
        check('move: a download can be moved to another queue', 'err' not in mv and task(paused_t['id'])['queue_id'] == 'main', mv)
        ui.invoke('move_to_queue_cmd', {'ids': [paused_t['id']], 'queueId': dq['id']})
        ui.invoke('delete_queue_cmd', {'id': dq['id']})
        check('delete: its downloads move to the main queue', task(paused_t['id'])['queue_id'] == 'main' and q(dq['id']) is None, task(paused_t['id']))
        ui.invoke('delete_download_cmd', {'id': paused_t['id'], 'deleteFile': True})

        # ---- unknown queue falls back to main
        t8, _ = add('small.bin', 'does-not-exist', filename='fallback.bin')
        check('add: an unknown queue falls back to the main queue', bool(t8) and t8['queue_id'] == 'main', t8)

        # ---- queues survive a restart
        names_before = sorted(x['name'] for x in queues())
    finally:
        app.kill()
        time.sleep(1)

    # restart and check persistence
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
        ui = App(WS(page['webSocketDebuggerUrl']))
        time.sleep(2)
        after = ui.invoke('get_queues_cmd').get('ok') or []
        check('restart: queues and their settings persist', sorted(x['name'] for x in after) == names_before and any(x['name'] == 'Retrying' and x['retry_count'] == 2 for x in after), [x['name'] for x in after])
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
