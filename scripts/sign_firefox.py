"""Gets the Firefox extension signed by Mozilla (unlisted: self-distributed).

    python scripts/sign_firefox.py

The Mozilla API keys (AMO_JWT_ISSUER / AMO_JWT_SECRET, from
addons.mozilla.org/developers/addon/api/key/) are read from the environment or
from the git-ignored file .env.amo in the repo root.

What happens: the Firefox package is built, uploaded to Mozilla, checked
automatically (minutes, usually), and the signed .xpi is saved to
extensions/signed/nexdrop-firefox.xpi. The installer and the app pick it up
from there. Every upload needs a version number Mozilla hasn't seen: raise
"version" in extensions/nexdrop-browser-extension/manifest.json first.

See docs/browser-extension-publishing.md for the one-time account setup.
"""
import base64, glob, hashlib, hmac, json, os, shutil, subprocess, sys, tempfile, time, urllib.parse, urllib.request, uuid

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
signed_dir = os.path.join(root, 'extensions', 'signed')


def load_env_file():
    """Reads AMO_JWT_* from the git-ignored .env.amo in the repo root, if present."""
    path = os.path.join(root, '.env.amo')
    if not os.path.exists(path):
        return
    for line in open(path, encoding='utf-8'):
        key, sep, value = line.strip().partition('=')
        if sep and key.startswith('AMO_') and key not in os.environ:
            os.environ[key] = value


def jwt(issuer, secret):
    """The short-lived token Mozilla's API wants (HS256, valid for a few minutes)."""
    b64 = lambda raw: base64.urlsafe_b64encode(raw).rstrip(b'=')
    head = b64(json.dumps({'alg': 'HS256', 'typ': 'JWT'}).encode())
    now = int(time.time())
    body = b64(json.dumps({'iss': issuer, 'jti': str(uuid.uuid4()), 'iat': now, 'exp': now + 240}).encode())
    sig = b64(hmac.new(secret.encode(), head + b'.' + body, hashlib.sha256).digest())
    return (head + b'.' + body + b'.' + sig).decode()


def download_existing(issuer, secret, addon_id, version, target):
    """The version was already uploaded (an earlier run lost its connection while waiting):
    fetch the file Mozilla signed for it."""
    api = 'https://addons.mozilla.org/api/v5/addons/addon/' + urllib.parse.quote(addon_id, safe='') + '/versions/?filter=all_with_unlisted'
    def get(url):
        req = urllib.request.Request(url, headers={'Authorization': 'JWT ' + jwt(issuer, secret)})
        return urllib.request.urlopen(req, timeout=60).read()
    for _ in range(30):  # Mozilla may still be checking it
        for v in json.loads(get(api)).get('results', []):
            f = v.get('file') or {}
            if v.get('version') == version and f.get('status') in ('public', 'approved', 'unreviewed') and f.get('url'):
                with open(target, 'wb') as out:
                    out.write(get(f['url']))
                return True
        time.sleep(10)
    return False


def main():
    load_env_file()
    issuer = os.environ.get('AMO_JWT_ISSUER')
    secret = os.environ.get('AMO_JWT_SECRET')
    if not issuer or not secret:
        print('Set AMO_JWT_ISSUER and AMO_JWT_SECRET first (see docs/browser-extension-publishing.md).')
        return 2

    work = tempfile.mkdtemp(prefix='nexdrop_sign_')
    subprocess.run([sys.executable, os.path.join(root, 'scripts', 'package_extension.py'), work], check=True, stdout=subprocess.DEVNULL)
    source = os.path.join(work, 'firefox')
    version = json.load(open(os.path.join(source, 'manifest.json'), encoding='utf-8'))['version']
    artifacts = os.path.join(work, 'artifacts')
    print(f'Uploading version {version} to Mozilla for signing (unlisted)...')

    npx = shutil.which('npx') or shutil.which('npx.cmd')
    if not npx:
        print('Node.js (npx) is needed: https://nodejs.org')
        return 2
    result = subprocess.run(
        [npx, '--yes', 'web-ext@8', 'sign', '--channel=unlisted', f'--source-dir={source}', f'--artifacts-dir={artifacts}',
         f'--api-key={issuer}', f'--api-secret={secret}', '--approval-timeout=1800000'],
    )
    xpis = glob.glob(os.path.join(artifacts, '*.xpi'))
    if result.returncode != 0 and not xpis:
        # The upload may have gone through even though the connection dropped afterwards.
        os.makedirs(artifacts, exist_ok=True)
        recovered = os.path.join(artifacts, f'nexdrop-{version}.xpi')
        try:
            if download_existing(issuer, secret, 'nexdrop@nexdrop.app', version, recovered):
                print(f'Version {version} was already signed by Mozilla; downloaded that file.')
                xpis = [recovered]
        except Exception as e:  # noqa: BLE001
            print('Could not fetch an earlier signing of this version:', e)
    if result.returncode != 0 and not xpis:
        print('Mozilla did not sign the add-on. The message above says why (common: the version was already used).')
        return result.returncode

    if not xpis:
        print('Signing finished but no .xpi was produced.')
        return 1
    os.makedirs(signed_dir, exist_ok=True)
    target = os.path.join(signed_dir, 'nexdrop-firefox.xpi')
    shutil.copy2(xpis[0], target)
    with open(os.path.join(signed_dir, 'VERSION'), 'w', encoding='utf-8') as f:
        f.write(version + '\n')
    shutil.rmtree(work, ignore_errors=True)
    print(f'Signed. Saved to {target}')
    print('Rebuild the installer (scripts/release.sh) to include it.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
