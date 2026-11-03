"""The "several links" window in the REAL app, fed the way the browser extension feeds it.

    python scripts/e2e_many_links.py

Sends a page's links (files, a nameless download link whose text names the file, and
ordinary pages) to the app's bridge like "Download all files on this page" does, then
checks the window that opens: files first, the nameless link named from its text, the
pages tucked away behind "Show other links". Close Nexdrop first.
"""
import json, os, subprocess, sys, time, urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'scripts'))
from e2e_torrent import WS, App

EXE = os.path.join(ROOT, 'src-tauri', 'target', 'release', 'nexdrop.exe')
DEBUG_PORT = 9231
results = []


def check(name, ok, detail=''):
    results.append((name, ok))
    print(('PASS  ' if ok else 'FAIL  ') + name + (f'  [{detail}]' if detail and not ok else ''), flush=True)


def post(path, body):
    req = urllib.request.Request('http://127.0.0.1:18888' + path, data=json.dumps(body).encode(), method='POST')
    req.add_header('Origin', 'chrome-extension://e2e')
    req.add_header('Content-Type', 'application/json')
    with urllib.request.urlopen(req, timeout=10) as r:
        return r.status, json.loads(r.read() or b'{}')


def main():
    try:
        urllib.request.urlopen('http://127.0.0.1:18888/status', timeout=1)
        print('Nexdrop is running; close it first.')
        return 2
    except Exception:
        pass
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
        time.sleep(2.5)
        text = lambda: ws.call('Runtime.evaluate', {'expression': 'document.body.innerText', 'returnByValue': True})['result']['result']['value']

        site = 'https://files.example.com'
        items = [
            {'url': f'{site}/about', 'text': 'About us'},
            {'url': f'{site}/get?id=5', 'text': 'Setup-2.1.exe'},
            {'url': f'{site}/music/song-one.mp3', 'text': 'song one'},
            {'url': f'{site}/blog', 'text': 'Blog'},
            {'url': f'{site}/dl/archive.zip', 'text': 'Download'},
        ]
        status, body = post('/download-many', {'urls': [i['url'] for i in items], 'items': items, 'mode': 'page', 'referer': site + '/'})
        check('bridge: the links are accepted', status == 200 and body.get('count') == 5, body)
        time.sleep(1.5)
        t = text()
        check('window: opens with the links', 'Add multiple downloads' in t, t[:200])
        check('window: a nameless link is named from its text', 'Setup-2.1.exe' in t, t[:400])
        check('window: files are listed', 'song-one.mp3' in t and 'archive.zip' in t, t[:400])
        check('window: ordinary pages are tucked away', 'about' not in t.lower().split('show')[0] and 'Show 2 other links' in t, t[:500])
        check('window: the three files are preselected', 'Add 3 downloads' in t, t[-300:])
        ws.call('Runtime.evaluate', {'expression': "[...document.querySelectorAll('button')].find(b => b.textContent.startsWith('Show 2 other'))?.click()"})
        time.sleep(0.5)
        t = text()
        check('window: other links show on request, not selected', 'Hide other links' in t and 'Add 3 downloads' in t, t[-400:])
    finally:
        app.kill()
        time.sleep(1)
    failed = [n for n, ok in results if not ok]
    print(f'\n{len(results) - len(failed)}/{len(results)} checks passed')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
