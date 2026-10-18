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


def chord(mod, key):
    ActionChains(d).key_down(mod).send_keys(key).key_up(mod).perform()


def click_text(text, tag='button'):
    for el in d.find_elements(By.TAG_NAME, tag):
        if el.text.strip() == text and el.is_displayed():
            el.click()
            return True
    return False


try:
    d.get(URL)
    time.sleep(2.5)
    shot('downloads')

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
finally:
    d.quit()
