"""Builds the browser extension packages.

    python scripts/package_extension.py [output-folder]

Writes, into the output folder (default release/extension):

  chromium/   unpacked extension for Chrome, Edge, Brave (manifest without Firefox keys)
  firefox/    unpacked extension for Firefox (manifest without the service worker,
              with the add-on id and the data-collection declaration)
  nexdrop-chromium-<version>.zip   what the Chrome Web Store / Edge Add-ons accept
  nexdrop-firefox-<version>.zip    what Mozilla's add-on site accepts (before signing)

The sources in extensions/nexdrop-browser-extension carry both browsers' keys so
the folder loads anywhere during development; the packages here are the clean,
store-ready variants.
"""
import json, os, shutil, sys, zipfile

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
src = os.path.join(root, 'extensions', 'nexdrop-browser-extension')

# What the extension sends to the Nexdrop app running on the same computer, in
# Mozilla's vocabulary. Nothing is sent to any server of ours.
FIREFOX_DATA_COLLECTION = {'required': ['browsingActivity', 'authenticationInfo']}


def manifest_for(kind, base):
    m = json.loads(json.dumps(base))
    bg = m.get('background', {})
    if kind == 'chromium':
        m['background'] = {'service_worker': bg.get('service_worker', 'background.js')}
        m.pop('browser_specific_settings', None)
    else:
        m['background'] = {'scripts': bg.get('scripts', ['background.js'])}
        gecko = m.setdefault('browser_specific_settings', {}).setdefault('gecko', {})
        gecko['id'] = gecko.get('id', 'nexdrop@nexdrop.app')
        gecko['strict_min_version'] = '140.0'  # data_collection_permissions needs Firefox 140+
        gecko['data_collection_permissions'] = FIREFOX_DATA_COLLECTION
    return m


def build(kind, out, base):
    folder = os.path.join(out, kind)
    shutil.rmtree(folder, ignore_errors=True)
    shutil.copytree(src, folder)
    manifest = manifest_for(kind, base)
    with open(os.path.join(folder, 'manifest.json'), 'w', encoding='utf-8') as f:
        json.dump(manifest, f, indent=2, ensure_ascii=False)
        f.write('\n')
    zip_path = os.path.join(out, f'nexdrop-{kind}-{base["version"]}.zip')
    with zipfile.ZipFile(zip_path, 'w', zipfile.ZIP_DEFLATED) as z:
        for current, _, files in os.walk(folder):
            for name in sorted(files):
                full = os.path.join(current, name)
                z.write(full, os.path.relpath(full, folder).replace(os.sep, '/'))
    return folder, zip_path


def main():
    base = json.load(open(os.path.join(src, 'manifest.json'), encoding='utf-8'))
    out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(root, 'release', 'extension')
    os.makedirs(out, exist_ok=True)
    for kind in ('chromium', 'firefox'):
        folder, zip_path = build(kind, out, base)
        print('wrote', folder)
        print('wrote', zip_path)


if __name__ == '__main__':
    main()
