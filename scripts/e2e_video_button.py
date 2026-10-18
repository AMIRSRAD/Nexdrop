"""The video button must stay out of the way. Real Edge and Firefox, no Nexdrop needed.

    python scripts/e2e_video_button.py [edge] [firefox]
"""
import os, sys, threading, time, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import e2e_extension as t
from selenium.webdriver.common.action_chains import ActionChains
from selenium.webdriver.common.by import By

site = t.Threaded(('127.0.0.1', 0), t.Site)
threading.Thread(target=site.serve_forever, daemon=True).start()
base = f'http://127.0.0.1:{site.server_address[1]}'
for kind in sys.argv[1:] or ['edge', 'firefox']:
    print(f'=== {kind} ===', flush=True)
    d = t.start_browser(kind, tempfile.mkdtemp())
    try:
        time.sleep(3)
        d.get(base + '/plainvideo.html')
        time.sleep(2)
        video = d.find_element(By.ID, 'v')
        state = lambda: d.execute_script("var h = document.querySelector('[data-nexdrop]'); return h && h.getAttribute('data-nexdrop')")
        t.check(f'{kind}: starts hidden', state() == 'hidden', state())
        ActionChains(d).move_to_element(video).perform()
        time.sleep(0.2)
        ActionChains(d).move_by_offset(30, 20).perform()
        t.check(f'{kind}: appears when you move over a video', t.wait_for(lambda: state() == 'visible', timeout=3) is True, state())
        time.sleep(4.5)
        t.check(f'{kind}: goes away by itself when the pointer rests', state() == 'hidden', state())
        ActionChains(d).move_by_offset(-20, 10).perform()
        t.check(f'{kind}: comes back when the pointer moves again', t.wait_for(lambda: state() == 'visible', timeout=3) is True, state())
        ActionChains(d).move_to_element(d.find_element(By.ID, 'away')).perform()
        t.check(f'{kind}: goes away when the pointer leaves the video', t.wait_for(lambda: state() == 'hidden', timeout=4) is True, state())
    finally:
        d.quit()
failed = [n for n, ok in t.results if not ok]
print(f'{len(t.results) - len(failed)}/{len(t.results)} checks passed')
sys.exit(1 if failed else 0)
