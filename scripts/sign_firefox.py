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
import glob, json, os, shutil, subprocess, sys, tempfile

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
    if result.returncode != 0:
        print('Mozilla did not sign the add-on. The message above says why (common: the version was already used).')
        return result.returncode

    xpis = glob.glob(os.path.join(artifacts, '*.xpi'))
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
