"""Installs the Mozilla-signed add-on in real Firefox the permanent way and checks it connects.

    python scripts/verify_signed_xpi.py

Uses a fake Nexdrop bridge on :18888 (close Nexdrop first) and a throwaway Firefox profile.
"""
import os, sys, threading, time, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import e2e_extension as t
from selenium import webdriver
from selenium.webdriver.firefox.options import Options
from selenium.webdriver.firefox.service import Service

xpi = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'extensions', 'signed', 'nexdrop-firefox.xpi')
bridge = t.Threaded(('127.0.0.1', 18888), t.Bridge)
threading.Thread(target=bridge.serve_forever, daemon=True).start()
o = Options(); o.add_argument('-headless')
d = webdriver.Firefox(options=o, service=Service(service_args=['--allow-system-access']))
try:
    addon_id = d.install_addon(xpi, temporary=False)  # a permanent install: only works if Mozilla signed it
    print('installed as', addon_id)
    hello = t.wait_for(lambda: [p for p in t.POSTS if p.get('path') == '/hello'], timeout=20)
    print('hello from the signed add-on:', [(h.get('browser'), h.get('version')) for h in hello])
    d.set_context('chrome')
    info = d.execute_async_script("const cb = arguments[arguments.length-1]; ChromeUtils.importESModule('resource://gre/modules/AddonManager.sys.mjs').AddonManager.getAddonByID(arguments[0]).then(a => cb(a && {name: a.name, signed: a.signedState, state: a.signedState === 2 ? 'privileged' : 'signed-or-not', temporary: a.temporarilyInstalled, active: a.isActive, version: a.version}));", addon_id)
    print('Firefox says:', info)
    ok = bool(hello) and info and not info['temporary'] and info['active'] and info['signed'] in (2, 3)
    print('RESULT', 'PASS' if ok else 'FAIL')
    sys.exit(0 if ok else 1)
finally:
    d.quit(); bridge.shutdown()
