# Nexdrop roadmap: closing the gap with IDM

Ground rules for every item: **the interface stays clean**. New features go into
places that already exist (settings, the row menu, dialogs) or appear only
when they are relevant; nothing permanent is added to the main window unless
people reach for it constantly.

Status: `[x]` done, `[ ]` to do, `[~]` partly done.

## 1. Scheduler and queues

- [x] Named queues that you create, rename and delete (a built-in Main queue always exists)
- [x] Assign downloads to a queue: Add dialog, row menu "Move to queue", and "Move to queue" for a selection
- [x] "Add to queue" in the New download dialog: choose the queue and add without starting
- [x] Choose which network adapters downloads use, or several at once (Settings → Connections)
- [x] Per-queue schedule: always on, or on chosen days with a start time and an optional stop time
- [x] Start now / Stop for a queue, regardless of its schedule
- [x] Per-queue limits: simultaneous downloads and a shared speed limit
- [x] Per-queue "when finished" action: nothing, quit Nexdrop, sleep, hibernate, shut down, lock, or run a program
- [x] Cancelable countdown before sleep / hibernate / shut down
- [x] Per-queue automatic retry of failed downloads (count and delay)
- [x] Queues always in view: a side panel lists them with what each is doing, filters the list, and starts / stops a queue in place
- [x] Manage the line like IDM: Download next, move up / down / to the end (menu, Alt+arrows), Download now past a full queue, drag downloads onto a queue; "Next up" / "#N in line" labels; the order survives restarts
- [x] Bulk actions: Clear completed, Retry all, Pause all, Resume all (a button on the matching tab, and a ⋯ menu), Remove failed
- [x] Existing scheduler settings migrate into the Main queue
- [x] Add-dialog scheduling ("download later") keeps working

## 2. Streaming video (HLS / DASH)

- [x] Detect `.m3u8` and `.mpd` playlists in the browser extension (network-level, not only `<video>` tags)
- [x] Download HLS media playlists (segments fetched in parallel, AES-128 decryption, byte ranges, resume from the last whole segment)
- [x] Choose a quality from a master playlist
- [x] Merge segments into one playable file: transport streams become one `.ts`; separate video and audio fragmented-MP4 tracks are joined into one `.mp4` without re-encoding
- [x] DASH (`.mpd`) manifests (on-demand streams: segment templates, timelines, segment lists)
- [~] Video button on the page: appears on videos and sends the detected stream; the quality picker is in Nexdrop's Add dialog, which opens by itself
- [ ] Live streams (record until the stream ends)
- [ ] Join separate audio for transport-stream HLS (needs re-muxing; the audio is saved next to the video as its own file for now)
- [ ] Subtitle tracks (WebVTT)
- Not planned: DRM-protected streams (Widevine / FairPlay / SAMPLE-AES): they are detected and refused with a clear message

## 3. Proxy depth

- [x] SOCKS5 proxies (names resolved through the proxy)
- [x] Proxy username and password (password encrypted on disk)
- [ ] PAC script URL
- [ ] Bypass list (`localhost; *.local; 192.168.*`)
- [ ] Per-protocol proxy (HTTP, HTTPS)
- [x] Proxy test button ("Test connection", shows the address websites see)

## 4. Browser integration

- [x] Downloads the browser starts go to Nexdrop, with the page's cookies, referer and user agent (Chrome, Edge, Firefox)
- [x] The download window opens first, like IDM (setting: "Show the download window first")
- [x] Settings for which file types are captured, and a minimum size for other files (extension options page)
- [x] Never-intercept list of sites
- [x] Alt-click leaves a link to the browser
- [x] Videos and PDFs you click download instead of opening in the tab
- [x] Download button over playing videos, finding HLS/DASH streams and video files the page loaded, with a picker when there are several
- [x] Panel listing the videos and files on the current page (toolbar popup, with a badge count)
- [x] Video button only over real videos (no muted loops or short clips), only when something can be downloaded, and a × to hide it for the page
- [x] Right-click: "Download all N files on this page" (file links only) and "Download N selected links", shown only when they apply; links named from their text
- [x] "Find the download": a link that is a web page with a Download button (file hosts); Nexdrop opens it in its own view, presses the button and takes the file (only when pressed)
- [x] Firefox support (same package; verified in real Firefox and Edge by `scripts/e2e_extension.py`)
- [x] First-run setup: Nexdrop finds Chrome, Edge, Brave and Firefox, opens each one's extensions page, shows what to click and waits until the browser connects
- [x] The app shows which browsers are connected (Settings > Browser, status bar nudge until one is set up)
- [x] The Firefox extension is signed by Mozilla (unlisted) with `python scripts/sign_firefox.py` (see docs/browser-extension-publishing.md); the installer bundles the signed add-on
- [ ] Publish the extension to the Chrome Web Store, Edge Add-ons and Mozilla Add-ons (needed for one-click install and for Firefox to keep it after restarts)
- [ ] POST-based downloads (forms that return a file)
- [ ] Start Nexdrop automatically when the browser sends a download and it isn't running

## 5. Failed and expired downloads

- [x] "Refresh link": paste a new link onto a failed or expired download and resume it
- [x] Automatic retry of failed downloads (see Queues)
- [ ] Re-fetch an expired link from the page it came from (extension; "Find the download" does this by hand today)

## 6. Download dialogs and prompts

- [x] "File already exists": keep both or replace
- [x] Duplicate link detection when adding (single and multiple)
- [ ] "Download complete" prompt: Open, Open folder (optional, off by default)
- [ ] Live per-download details window with the segment map
- [ ] Remember the last folder per file type
- [ ] Custom temp folder for partial files
- [ ] Connection presets (slow / medium / fast)

## 7. Per-site connection limits

- [x] Make "max connections per server" actually limit connections per host (shared fairly between downloads)
- [ ] Per-site overrides (connections, speed)

## 8. Sites and protocols

- [ ] Site Grabber: crawl a site with depth and file-type filters
- [ ] FTP / FTPS downloads
- [ ] Save and load grabber projects

## 9. Data and recovery

- [ ] Import / export the download list
- [ ] Automatic backup of the download list
- [ ] Completed-downloads history cleanup rules

## 10. Polish

- [ ] Configurable keyboard shortcuts
- [ ] Speed-limit toggle in the tray
- [ ] Per-event sounds
- [ ] More languages (UI strings extracted first)

## 11. Reliability, safety and upkeep (done)

- [x] Every finished download of 16 MiB or more is checked against the server before it is kept; wrong parts are downloaded again ("Verifying…" in the list)
- [x] Ranges are cut on 4 MiB boundaries and mid-slice requests are avoided (a CDN bug that shifted data), see ENGINE_REPORT.md
- [x] Activity log per download and "Copy diagnostics"
- [x] Add multiple returns at once: links are checked in the background, appear in order, failures are reported by address
- [x] Removing an unfinished download always removes its temporary data, without asking
- [x] Signed updates from GitHub releases, offered inside the app (Settings → About)
- [x] Light when idle: low memory while hidden in the tray, no polling while hidden, cheap background checks
- [x] Network adapters: use one connection or several at once (Settings → Connections)
- [x] About page and licence
- [ ] Windows installer code signing (SmartScreen still warns)
- [ ] macOS and Linux builds tested on real machines (they are built and unit-tested in CI only)
