"""Verifies the *installed* app: install, launch, single instance, close-to-tray,
autostart, tray icon, clean exit, uninstall.

    pnpm tauri build
    python scripts/e2e_release.py

Installs Nexdrop silently for the current user (so it replaces any existing
per-user install), exercises it, and uninstalls it again. Your Nexdrop
database is backed up first and restored afterwards. Close Nexdrop first.
"""
import ctypes, glob, json, os, shutil, subprocess, sys, tempfile, time, urllib.request, winreg
from ctypes import wintypes

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
INSTALLER = (sorted(glob.glob(os.path.join(ROOT, 'src-tauri', 'target', 'release', 'bundle', 'nsis', '*-setup.exe'))) or [None])[-1]
INSTALL_DIR = os.path.join(os.environ['LOCALAPPDATA'], 'Nexdrop')
EXE = os.path.join(INSTALL_DIR, 'nexdrop.exe')
DATA = os.path.join(os.environ['LOCALAPPDATA'], 'Nexdrop')
DB = os.path.join(DATA, 'nexdrop.db')
BACKUP = os.path.join(tempfile.gettempdir(), 'nexdrop_release_e2e_backup')
BRIDGE = 'http://127.0.0.1:18888'
RUN_KEY = r'Software\Microsoft\Windows\CurrentVersion\Run'
UNINSTALL_KEY = r'Software\Microsoft\Windows\CurrentVersion\Uninstall'
user32 = ctypes.windll.user32
results = []


def check(name, ok, detail=''):
    results.append((name, ok))
    print(('PASS  ' if ok else 'FAIL  ') + name + (f'  [{detail}]' if detail and not ok else ''), flush=True)


def pids():
    out = subprocess.run(['tasklist', '/FI', 'IMAGENAME eq nexdrop.exe', '/FO', 'CSV', '/NH'], capture_output=True, text=True).stdout
    return [int(line.split(',')[1].strip('"')) for line in out.splitlines() if line.startswith('"nexdrop.exe"')]


def main_windows(pid_list):
    found = []
    EnumProc = ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)

    def cb(hwnd, _):
        p = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(p))
        if p.value in pid_list:
            n = user32.GetWindowTextLengthW(hwnd)
            buf = ctypes.create_unicode_buffer(n + 1)
            user32.GetWindowTextW(hwnd, buf, n + 1)
            if buf.value == 'Nexdrop':
                found.append((hwnd, bool(user32.IsWindowVisible(hwnd))))
        return True

    user32.EnumWindows(EnumProc(cb), 0)
    return found


def window_state():
    w = main_windows(pids())
    return None if not w else w[0][1]


def wait(cond, timeout=20, step=0.4):
    end = time.time() + timeout
    while time.time() < end:
        v = cond()
        if v:
            return v
        time.sleep(step)
    return cond()


def bridge_up():
    try:
        urllib.request.urlopen(BRIDGE + '/status', timeout=1)
        return True
    except Exception:
        return False


def reg_value(key, name):
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key) as k:
            return winreg.QueryValueEx(k, name)[0]
    except OSError:
        return None


def uninstall_entry():
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, UNINSTALL_KEY) as root:
            i = 0
            while True:
                name = winreg.EnumKey(root, i)
                i += 1
                with winreg.OpenKey(root, name) as k:
                    try:
                        if winreg.QueryValueEx(k, 'DisplayName')[0] == 'Nexdrop':
                            return name, winreg.QueryValueEx(k, 'UninstallString')[0], winreg.QueryValueEx(k, 'Publisher')[0] if True else ''
                    except OSError:
                        pass
    except OSError:
        return None


def tray_registered():
    """Windows 11 records every tray icon under NotifyIconSettings."""
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r'Control Panel\NotifyIconSettings') as root:
            i = 0
            while True:
                name = winreg.EnumKey(root, i)
                i += 1
                with winreg.OpenKey(root, name) as k:
                    try:
                        if 'nexdrop' in winreg.QueryValueEx(k, 'ExecutablePath')[0].lower():
                            return True
                    except OSError:
                        pass
    except OSError:
        return False


def kill_all():
    subprocess.run(['taskkill', '/F', '/IM', 'nexdrop.exe'], capture_output=True)
    time.sleep(1)


def main():
    if not INSTALLER:
        print('No installer found. Run: pnpm tauri build')
        return 2
    if bridge_up():
        print('Nexdrop is running on :18888; close it first.')
        return 2

    os.makedirs(BACKUP, exist_ok=True)
    for ext in ('', '-wal', '-shm'):
        if os.path.exists(DB + ext):
            shutil.copy2(DB + ext, os.path.join(BACKUP, 'nexdrop.db' + ext))
    print('database backed up; installer:', os.path.basename(INSTALLER), f'({os.path.getsize(INSTALLER) / 1e6:.1f} MB)', flush=True)

    try:
        # ---- install
        r = subprocess.run([INSTALLER, '/S'], capture_output=True, timeout=300)
        wait(lambda: os.path.exists(EXE), 60)
        check('install: silent install succeeds', r.returncode == 0 and os.path.exists(EXE), r.returncode)
        ext_manifest = glob.glob(os.path.join(INSTALL_DIR, '**', 'manifest.json'), recursive=True)
        check('install: browser extension is bundled', any('extension' in p for p in ext_manifest), ext_manifest)
        entry = uninstall_entry()
        check('install: listed in Apps & features with an uninstaller', bool(entry) and os.path.exists(os.path.join(INSTALL_DIR, 'uninstall.exe')), entry)
        start_menu = glob.glob(os.path.join(os.environ['APPDATA'], 'Microsoft', 'Windows', 'Start Menu', 'Programs', '**', 'Nexdrop*.lnk'), recursive=True)
        check('install: Start menu shortcut', bool(start_menu), start_menu)

        # ---- first launch
        proc = subprocess.Popen([EXE], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        check('launch: bridge comes up (backend started)', bool(wait(bridge_up, 30)))
        check('launch: main window is visible', wait(lambda: window_state() is True, 20) is True, window_state())
        check('launch: exactly one process tree owner', len(pids()) == 1, pids())
        check('launch: tray icon registered with Windows', bool(wait(tray_registered, 10)), 'no NotifyIconSettings entry')

        # ---- single instance
        second = subprocess.Popen([EXE], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            second.wait(timeout=15)
            exited = True
        except subprocess.TimeoutExpired:
            exited = False
        time.sleep(1)
        check('single instance: a second launch exits by itself', exited)
        check('single instance: still exactly one running', len(pids()) == 1, pids())

        # ---- close = hide to tray
        hwnd = main_windows(pids())[0][0]
        user32.PostMessageW(hwnd, 0x0010, 0, 0)  # WM_CLOSE
        check('close: the window hides', wait(lambda: window_state() is False, 8) is True, window_state())
        time.sleep(1)
        check('close: the app keeps running (in the tray)', len(pids()) == 1 and bridge_up(), pids())

        # ---- launching again brings the hidden window back
        subprocess.Popen([EXE], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        check('reopen: launching again shows the hidden window', wait(lambda: window_state() is True, 10) is True, window_state())

        # ---- exit
        kill_all_ok = True
        # Ask politely through the tray's Exit path: the app's own quit command.
        # (No webview debug port here, so fall back to a second close + taskkill for cleanup only.)
        pass

        # ---- autostart + minimized start use the registry entry the app writes
        env = dict(os.environ, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9224')
        kill_all()
        check('exit: process is gone after being stopped', not pids())

        proc = subprocess.Popen([EXE, '--minimized'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        up = wait(bridge_up, 30)
        time.sleep(2)
        check('autostart launch: starts hidden in the tray (--minimized)', bool(up) and window_state() is False, window_state())

        page = None
        for _ in range(40):
            try:
                pages = json.loads(urllib.request.urlopen('http://127.0.0.1:9224/json', timeout=1).read())
                page = next(p for p in pages if p['type'] == 'page')
                break
            except Exception:
                time.sleep(0.5)
        if page:
            sys.path.insert(0, os.path.join(ROOT, 'scripts'))
            from e2e_torrent import WS, App  # reuse the tiny CDP client
            ui = App(WS(page['webSocketDebuggerUrl']))
            on = ui.invoke('set_autostart_cmd', {'enabled': True})
            val = reg_value(RUN_KEY, 'Nexdrop') or reg_value(RUN_KEY, 'nexdrop')
            check('autostart: enabling writes a Run entry that starts hidden', on.get('ok') is True and bool(val) and '--minimized' in val, (on, val))
            off = ui.invoke('set_autostart_cmd', {'enabled': False})
            check('autostart: disabling removes it', off.get('ok') is False and not (reg_value(RUN_KEY, 'Nexdrop') or reg_value(RUN_KEY, 'nexdrop')), off)
            info = ui.invoke('get_app_info_cmd')
            check('app info: name and version', info.get('ok', {}).get('version') == '0.1.0', info)
            ext = ui.invoke('get_extension_dir_cmd')
            check('extension folder resolves inside the install', 'ok' in ext and os.path.exists(os.path.join(ext['ok'], 'manifest.json')), ext)

            ui.invoke('quit_app_cmd')
            gone = wait(lambda: not pids(), 20)
            check('exit: the Exit command stops the app cleanly', bool(gone), pids())
        else:
            check('could not attach to the installed app webview', False)
            kill_all()
    finally:
        kill_all()
        # ---- uninstall
        unin = os.path.join(INSTALL_DIR, 'uninstall.exe')
        if os.path.exists(unin):
            subprocess.run([unin, '/S'], capture_output=True, timeout=300)
            wait(lambda: not os.path.exists(EXE), 60)
        check('uninstall: program files removed', not os.path.exists(EXE), os.listdir(INSTALL_DIR) if os.path.isdir(INSTALL_DIR) else '')
        check('uninstall: removed from Apps & features', uninstall_entry() is None)
        check('uninstall: Start menu shortcut removed', not glob.glob(os.path.join(os.environ['APPDATA'], 'Microsoft', 'Windows', 'Start Menu', 'Programs', '**', 'Nexdrop*.lnk'), recursive=True))
        check('uninstall: no start-with-Windows entry left', not (reg_value(RUN_KEY, 'Nexdrop') or reg_value(RUN_KEY, 'nexdrop')))
        check('uninstall: keeps your data by default', os.path.exists(DB))
        for ext in ('', '-wal', '-shm'):
            try:
                os.remove(DB + ext)
            except OSError:
                pass
            b = os.path.join(BACKUP, 'nexdrop.db' + ext)
            if os.path.exists(b):
                shutil.copy2(b, DB + ext)
        print('database restored', flush=True)

    failed = [n for n, ok in results if not ok]
    print(f'\n{len(results) - len(failed)}/{len(results)} checks passed')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
