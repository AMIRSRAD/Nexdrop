"""The real app, a real browser and the real extension: does Nexdrop notice the connection?

    pnpm build && cargo build --release --manifest-path src-tauri/Cargo.toml
    pip install selenium
    python scripts/e2e_browser_connect.py [edge] [firefox]

Starts the built app, loads the extension into Edge and/or Firefox, and checks
the app's own view (the same command the interface uses) of which browsers are
installed and connected. Close Nexdrop first. The app's browser memory
(browsers_seen.json) is backed up and restored.
"""
import json, os, shutil, subprocess, sys, tempfile, time, urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'scripts'))
from e2e_torrent import WS, App  # tiny CDP client for the app
import e2e_extension as ext  # browser launching helpers

EXE = os.path.join(ROOT, 'src-tauri', 'target', 'release', 'nexdrop.exe')
DATA = os.path.join(os.environ['LOCALAPPDATA'], 'Nexdrop')
SEEN = os.path.join(DATA, 'browsers_seen.json')
DEBUG_PORT = 9228
BRIDGE = 'http://127.0.0.1:18888'
results = []


def check(name, ok, detail=''):
    results.append((name, ok))
    print(('PASS  ' if ok else 'FAIL  ') + name + (f'  [{detail}]' if detail and not ok else ''), flush=True)


def wait(cond, timeout=40, step=0.5):
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
        print('Nexdrop is already running on :18888; close it first.')
        return 2
    except Exception:
        pass
    kinds = sys.argv[1:] or ['edge', 'firefox']

    backup = None
    if os.path.exists(SEEN):
        backup = SEEN + '.e2e'
        shutil.copy2(SEEN, backup)
        os.remove(SEEN)

    env = dict(os.environ, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=f'--remote-debugging-port={DEBUG_PORT}')
    app = subprocess.Popen([EXE], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    drivers = []
    try:
        ui = None
        for _ in range(60):
            try:
                pages = json.loads(urllib.request.urlopen(f'http://127.0.0.1:{DEBUG_PORT}/json', timeout=1).read())
                page = next(p for p in pages if p['type'] == 'page' and 'tauri.localhost' in p['url'])
                ws = WS(page['webSocketDebuggerUrl'])
                ws.call('Runtime.enable')
                ui = App(ws)
                break
            except Exception:
                time.sleep(0.5)
        if not ui:
            print('could not attach to the app')
            return 1
        time.sleep(2)

        def browsers():
            r = ui.invoke('get_browsers_cmd')
            return (r.get('ok') or {}).get('browsers', []), r

        listed, raw = browsers()
        by_id = {b['id']: b for b in listed}
        for kind in kinds:
            check(f'{kind}: found on this computer', kind in by_id and by_id[kind]['installed'], raw)
            check(f'{kind}: not connected before the extension runs', kind in by_id and not by_id[kind]['connected'], by_id.get(kind))

        for kind in kinds:
            dl = tempfile.mkdtemp(prefix='nexdrop_conn_')
            driver = ext.start_browser(kind, dl)
            drivers.append(driver)
            seen = wait(lambda: next((b for b in browsers()[0] if b['id'] == kind and b['connected']), None), timeout=40)
            check(f'{kind}: the app sees the extension connect', bool(seen), browsers()[0])
            if seen:
                check(f'{kind}: and knows its version', bool(seen.get('version')), seen)
            shutil.rmtree(dl, ignore_errors=True)

        for d in drivers:
            try:
                d.quit()
            except Exception:
                pass
        drivers = []
        time.sleep(1)
        after, _ = browsers()
        for kind in kinds:
            b = next((x for x in after if x['id'] == kind), {})
            check(f'{kind}: remembered as set up once the browser is closed', b.get('ever_connected') is True, b)
        check('the app remembers it across restarts', os.path.exists(SEEN) and all(k in json.load(open(SEEN)) for k in kinds), os.path.exists(SEEN))
    finally:
        for d in drivers:
            try:
                d.quit()
            except Exception:
                pass
        app.kill()
        time.sleep(1)
        if os.path.exists(SEEN):
            os.remove(SEEN)
        if backup:
            shutil.move(backup, SEEN)

    failed = [n for n, ok in results if not ok]
    print(f'\n{len(results) - len(failed)}/{len(results)} checks passed')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
