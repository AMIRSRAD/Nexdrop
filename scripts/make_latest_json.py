"""Writes latest.json, the file Nexdrop's updater reads from the newest GitHub release.

    python scripts/make_latest_json.py <version> <installer folder>

Upload it to the release together with the installer and its .sig file.
"""
import datetime, glob, json, os, sys

version, folder = sys.argv[1], sys.argv[2]
exe = glob.glob(os.path.join(folder, f'*_{version}_*-setup.exe'))[0]
sig = open(exe + '.sig', encoding='utf-8').read().strip()
name = os.path.basename(exe)
latest = {
    'version': version,
    'notes': f'Nexdrop {version}',
    'pub_date': datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'),
    'platforms': {
        'windows-x86_64': {
            'signature': sig,
            'url': f'https://github.com/AMIRSRAD/Nexdrop/releases/download/v{version}/{name}',
        }
    },
}
out = os.path.join(folder, 'latest.json')
json.dump(latest, open(out, 'w', encoding='utf-8'), indent=2)
print('wrote', out)
