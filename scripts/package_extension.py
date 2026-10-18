"""Packages the browser extension for release.

    python scripts/package_extension.py [output-folder]

One package serves Chrome, Edge and Firefox (the manifest names both a service
worker and a background script). The same zip is also written as an .xpi,
which is the file type Firefox and its add-on store expect.
"""
import json, os, sys, zipfile

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
src = os.path.join(root, 'extensions', 'nexdrop-browser-extension')
manifest = json.load(open(os.path.join(src, 'manifest.json'), encoding='utf-8'))
version = manifest['version']
out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(root, 'release', 'extension')
os.makedirs(out, exist_ok=True)

zip_path = os.path.join(out, f'nexdrop-extension-{version}.zip')
with zipfile.ZipFile(zip_path, 'w', zipfile.ZIP_DEFLATED) as z:
    for folder, _, files in os.walk(src):
        for name in sorted(files):
            full = os.path.join(folder, name)
            z.write(full, os.path.relpath(full, src).replace(os.sep, '/'))
xpi_path = zip_path[:-4] + '.xpi'
with open(zip_path, 'rb') as a, open(xpi_path, 'wb') as b:
    b.write(a.read())
print('wrote', zip_path)
print('wrote', xpi_path)
