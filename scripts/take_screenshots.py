"""Captures the README screenshots from the running dev server (pnpm dev) using its demo data.

    pnpm dev            # in another terminal
    python scripts/take_screenshots.py
"""
import os, sys, time
from selenium import webdriver
from selenium.webdriver.common.action_chains import ActionChains
from selenium.webdriver.common.by import By
from selenium.webdriver.common.keys import Keys
from selenium.webdriver.edge.options import Options

OUT = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'docs', 'screenshots')
URL = 'http://localhost:5173/'
os.makedirs(OUT, exist_ok=True)

o = Options()
o.add_argument('--headless=new')
o.add_argument('--window-size=1360,860')
o.add_argument('--force-device-scale-factor=1')
d = webdriver.Edge(options=o)


def shot(name, pause=0.8):
    time.sleep(pause)
    d.save_screenshot(os.path.join(OUT, name + '.png'))
    print('saved', name)


def press(*keys):
    ActionChains(d).send_keys(*keys).perform()


def type_slow(text):
    # One key at a time: fast typing into the page can drop letters.
    for ch in text:
        ActionChains(d).send_keys(ch).perform()
        time.sleep(0.012)


def chord(mod, key):
    ActionChains(d).key_down(mod).send_keys(key).key_up(mod).perform()


def click_text(text, tag='button'):
    for el in d.find_elements(By.TAG_NAME, tag):
        if el.text.strip() == text and el.is_displayed():
            el.click()
            return True
    return False


def rows():
    return d.find_elements(By.CSS_SELECTOR, '[data-row-id]')


try:
    d.get(URL)
    time.sleep(2.5)
    shot('downloads')

    # Several rows selected: the bulk bar
    ActionChains(d).key_down(Keys.CONTROL).click(rows()[0]).click(rows()[2]).click(rows()[3]).key_up(Keys.CONTROL).perform()
    shot('select-multiple', pause=0.6)
    press(Keys.ESCAPE)

    # The bulk actions menu
    d.find_element(By.CSS_SELECTOR, '[aria-label="More actions"]').click()
    shot('bulk-actions', pause=0.6)
    press(Keys.ESCAPE)

    # Details of a running download: its Activity log (opened with a double-click)
    ActionChains(d).double_click(rows()[2]).perform()
    time.sleep(0.8)
    click_text('Activity')
    shot('details-activity', pause=0.8)
    press(Keys.ESCAPE)

    d.get(URL)  # a clean list behind the dialogs
    time.sleep(2)

    # New download: a normal file, with the queue and "Add to queue"
    chord(Keys.CONTROL, 'n')
    time.sleep(0.8)
    type_slow('https://releases.example.com/linux/ubuntu-24.04-desktop-amd64.iso')
    shot('add-download', pause=1.6)
    press(Keys.ESCAPE)

    d.get(URL)
    time.sleep(2)
    # New download: a web page with a Download button
    chord(Keys.CONTROL, 'n')
    time.sleep(0.8)
    type_slow('https://files.example.com/file/64842181')
    shot('find-the-download', pause=1.6)
    press(Keys.ESCAPE)

    d.get(URL)
    time.sleep(2)
    # Add multiple
    chord(Keys.CONTROL, 'b')
    time.sleep(0.8)
    type_slow(chr(10).join(['https://example.com/season1/episode-01.mp4', 'https://example.com/season1/episode-02.mp4', 'https://example.com/season1/episode-03.mp4', 'https://example.com/season1/subtitles.zip']))
    shot('add-multiple', pause=1.2)
    press(Keys.ESCAPE)

    # Add download: a streaming video with a quality picker
    chord(Keys.CONTROL, 'n')
    time.sleep(0.8)
    press('https://cdn.example.com/shows/Night%20Train/master.m3u8')
    shot('add-stream', pause=2.4)
    press(Keys.ESCAPE)

    # Settings pages
    chord(Keys.CONTROL, ',')
    time.sleep(1)
    click_text('Queues & schedule')
    time.sleep(0.8)
    click_text('New queue')
    time.sleep(0.8)
    for sw in d.find_elements(By.CSS_SELECTOR, '[role=switch]'):
        if 'schedule' in (sw.get_attribute('aria-label') or '').lower():
            sw.click()
            break
    shot('queues')

    click_text('Browser')
    time.sleep(1)
    shot('browser')

    click_text('Connect browsers…')
    shot('connect-browsers', pause=1)
    press(Keys.ESCAPE)

    click_text('Connections')
    shot('connections')

    click_text('Proxy')
    time.sleep(0.6)
    for lab in d.find_elements(By.TAG_NAME, 'label'):
        if 'Custom proxy' in lab.text:
            lab.click()
            break
    shot('proxy', pause=0.6)

    click_text('About')
    shot('about', pause=0.8)
finally:
    d.quit()
