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

- [ ] SOCKS4 / SOCKS5 proxies
- [ ] Proxy username and password
- [ ] PAC script URL
- [ ] Bypass list (`localhost; *.local; 192.168.*`)
- [ ] Per-protocol proxy (HTTP, HTTPS)
- [ ] Proxy test button ("Test connection")

## 4. Browser integration

- [x] Downloads the browser starts go to Nexdrop, with the page's cookies, referer and user agent (Chrome, Edge, Firefox)
- [x] The download window opens first, like IDM (setting: "Show the download window first")
- [x] Settings for which file types are captured, and a minimum size for other files (extension options page)
- [x] Never-intercept list of sites
- [x] Alt-click leaves a link to the browser
- [x] Videos and PDFs you click download instead of opening in the tab
- [x] Download button over playing videos, finding HLS/DASH streams and video files the page loaded, with a picker when there are several
- [x] Panel listing the videos and files on the current page (toolbar popup, with a badge count)
- [x] Firefox support (same package; verified in real Firefox and Edge by `scripts/e2e_extension.py`)
- [ ] Publish the extension to the Chrome Web Store, Edge Add-ons and Mozilla Add-ons (needed for one-click install and for Firefox to keep it after restarts)
- [ ] POST-based downloads (forms that return a file)
- [ ] Start Nexdrop automatically when the browser sends a download and it isn't running

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
