# Getting the browser extension signed and installed

Nexdrop's extension lives in `extensions/nexdrop-browser-extension`.

## Firefox: signed by Mozilla, distributed by you (unlisted)

Firefox only keeps extensions that Mozilla has signed. "Unlisted" means Mozilla
signs it automatically and you hand it out yourself: no public store page, no
human review.

One-time setup (about ten minutes):

1. Create a free account at <https://addons.mozilla.org> (a Firefox account works).
2. Open <https://addons.mozilla.org/developers/addon/api/key/> and press
   **Generate new credentials**. Copy the **JWT issuer** (looks like `user:12345:67`)
   and the **JWT secret**.

Each time you release:

1. Raise `version` in `extensions/nexdrop-browser-extension/manifest.json`
   (Mozilla refuses a version it has already seen).
2. In a terminal:
   ```
   set AMO_JWT_ISSUER=user:12345:67
   set AMO_JWT_SECRET=your-secret
   python scripts/sign_firefox.py
   ```
   Node.js must be installed. After a few minutes the signed file appears at
   `extensions/signed/nexdrop-firefox.xpi`.
3. Rebuild the installer (`bash scripts/release.sh`). It bundles the signed file.
   Commit the `.xpi` so everyone building gets it.

What Mozilla's automatic checks look at: the permissions (`cookies`, all sites,
downloads, web requests) and the data declaration. The manifest declares that the
extension handles browsing activity and authentication info (cookies), which it
sends only to the Nexdrop app on the same computer. If Mozilla's reviewer says data
that stays on the device doesn't need declaring, change
`FIREFOX_DATA_COLLECTION` in `scripts/package_extension.py` to
`{"required": ["none"]}`.

## Chrome and Edge: unpacked

These browsers load the extension from a folder (Developer mode, "Load unpacked").
Nexdrop's first-run setup opens the extensions page for you, shows the folder, and
tells you when the connection is working. Nothing has to be signed.

If you later want one-click installs for everyone, publish the `chromium` package
(`release/extension/nexdrop-chromium-<version>.zip`) to the Chrome Web Store and
Edge Add-ons, as *unlisted* if you don't want a public page.

## Building the packages

```
python scripts/package_extension.py release/extension
```
writes clean per-browser folders and zips (the sources carry both browsers' keys
so the folder also loads directly during development).
