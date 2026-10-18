# Nexdrop roadmap: closing the gap with IDM

Ground rules for every item: **the interface stays clean**. New features go into
places that already exist (settings, the row menu, dialogs) or appear only
when they are relevant; nothing permanent is added to the main window unless
people reach for it constantly.

Status: `[x]` done, `[ ]` to do, `[~]` partly done.

## 1. Scheduler and queues

- [x] Named queues that you create, rename and delete (a built-in Main queue always exists)
- [x] Assign downloads to a queue: Add dialog, row menu "Move to queue", and "Move to queue" for a selection
- [x] Per-queue schedule: always on, or on chosen days with a start time and an optional stop time
- [x] Start now / Stop for a queue, regardless of its schedule
- [x] Per-queue limits: simultaneous downloads and a shared speed limit
- [x] Per-queue "when finished" action: nothing, quit Nexdrop, sleep, hibernate, shut down, lock, or run a program
- [x] Cancelable countdown before sleep / hibernate / shut down
- [x] Per-queue automatic retry of failed downloads (count and delay)
- [x] Queue filter in the list (shown only once more than one queue exists)
- [x] Existing scheduler settings migrate into the Main queue
- [x] Add-dialog scheduling ("download later") keeps working

## 2. Streaming video (HLS / DASH)

- [ ] Detect `.m3u8` playlists in the browser extension (network-level, not only `<video>` tags)
- [ ] Download HLS media playlists (segments fetched in parallel, AES-128 decryption)
- [ ] Choose a quality from a master playlist
- [ ] Merge segments into one playable file (MP4/TS) without external tools
- [ ] DASH (`.mpd`) manifests
- [ ] Video grab bar shown on the page only when a stream is detected, with a quality picker

## 3. Proxy depth

- [ ] SOCKS4 / SOCKS5 proxies
- [ ] Proxy username and password
- [ ] PAC script URL
- [ ] Bypass list (`localhost; *.local; 192.168.*`)
- [ ] Per-protocol proxy (HTTP, HTTPS)
- [ ] Proxy test button ("Test connection")

## 4. Browser integration

- [ ] Settings for which file types are captured (currently hard-coded)
- [ ] Never-intercept list of sites
- [ ] Alt-click to force a download, Shift-click to skip
- [ ] Capture downloads that need cookies or a POST request
- [ ] Firefox build of the extension
- [ ] Panel listing the downloadable files on the current page

## 5. Failed and expired downloads

- [ ] "Refresh link": paste a new link onto a failed or expired download and resume it
- [x] Automatic retry of failed downloads (see Queues)
- [ ] Re-fetch an expired link from the page it came from (extension)

## 6. Download dialogs and prompts

- [ ] "File already exists": overwrite, rename or skip
- [ ] Duplicate link detection when adding
- [ ] "Download complete" prompt: Open, Open folder (optional, off by default)
- [ ] Live per-download details window with the segment map
- [ ] Remember the last folder per file type
- [ ] Custom temp folder for partial files
- [ ] Connection presets (slow / medium / fast)

## 7. Per-site connection limits

- [ ] Make "max connections per server" actually limit connections per host
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
