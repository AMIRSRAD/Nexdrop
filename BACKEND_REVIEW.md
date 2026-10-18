# Nexdrop Backend Review

**Scope:** everything except the interface. That covers the Rust engine (`src-tauri/src/**`), the SQLite layer, the Tauri commands, the browser bridge server, the scheduler, the tray, the IPC contract with the frontend, and the browser extension.
**Goals measured against:** (1) the fastest possible downloads, (2) safe downloads, (3) torrent support, (4) code that is free of bugs and performance problems.
**Date:** 2026-10-18. Line numbers refer to the current `main`.

---

## TL;DR — fix these first

| # | Severity | Problem | Where |
|---|----------|---------|-------|
| 1 | 🔴 Critical | **Downloads that take longer than 30 seconds fail.** `reqwest`'s `.timeout()` limits the *whole request, body included*, and it's set to 30 s. Any segment that streams for more than 30 s errors out, and one failed segment kills the whole download. | `engine/client.rs:13`, `engine/downloader.rs:38,61` |
| 2 | 🔴 Critical | **Resuming can corrupt files.** Segment files are opened in *append* mode, but per-segment progress isn't saved to the DB while downloading (only on pause). After a crash, a failure, or a pause race, the worker re-requests bytes the file already has and appends them again. The merged file comes out the wrong size and contains corrupted data. | `downloader.rs:265-289`, `db/repository.rs:199-222` |
| 3 | 🔴 Critical | **The queue never advances.** When a download finishes, nothing calls `process_queue()`. The 4th download (with the default of 3 active) waits forever until the user adds or resumes something. | `engine/manager.rs:280-281` |
| 4 | 🔴 Critical | **Any website can make Nexdrop download any file to any folder.** The bridge on `127.0.0.1:18888` has no authentication, sends `Access-Control-Allow-Origin: *`, and accepts an attacker-chosen `destination_path`. One drive-by request can drop `evil.exe` into the Startup folder. `/parse-page` also turns Nexdrop into an SSRF proxy for your LAN. | `grabber_server.rs:89,128-147,179-190,206` |
| 5 | 🔴 Critical | **Pausing is treated as completing.** The engine returns `Ok(task)` on pause. The manager then hashes and virus-scans a file that doesn't exist and emits `download-completed`. | `manager.rs:250-269`, `downloader.rs:136-137,381-388` |
| 6 | 🟠 High | **The app writes every byte twice.** Each segment is written to its own `.nexpart_seg_N` file, then all of them are copied into the final file. That doubles disk writes and disk space, and adds a long "frozen at 100%" phase on big files. | `downloader.rs:250,451-479` |
| 7 | 🟠 High | **The async runtime is blocked everywhere.** SQLite calls (opening a *new connection and running migrations every time*) run 4× per second per download inside the progress callback. SHA-256+MD5 of multi-GB files and the antivirus scan (`Command::output()`) run on Tokio worker threads. There is no `spawn_blocking` anywhere in the codebase. | `manager.rs:225-239,253,262`, `repository.rs:8-20` |
| 8 | 🟠 High | **There are no retries.** A single transient error (a connection reset or a 503) fails the entire download and cancels every healthy connection. The README promises exponential backoff, but it isn't implemented. | `downloader.rs:433-441` |
| 9 | 🟠 High | **Range responses aren't validated.** Workers accept `200 OK` to a `Range:` request, so a server that ignores ranges gets the *whole file* written into every segment. `ETag`/`If-Range` aren't checked on resume, so if the file changed on the server, you get a splice of two versions. | `downloader.rs:118-125,314` |
| 10 | 🟠 High | **The cmd window flash.** Every spawned process (Defender's `MpCmdRun.exe` after *every* completed download, `explorer`, `shutdown`, `rundll32`) is created without `CREATE_NO_WINDOW`. | `manager.rs:427,475`, `scheduler.rs:302,310` |

### Phase 1 status (branch `fix/backend-phase-1`)

| # | Status | What changed |
|---|--------|--------------|
| 1 | ✅ Fixed | Download clients have no total timeout; they use a 15 s connect timeout and a 30 s idle-read timeout (`engine/client.rs`). |
| 2 | ✅ Fixed | Segment files are opened without append, and the on-disk length is the source of truth on resume, so stale DB progress can't cause duplicate data. Surplus bytes are trimmed. Segments are never written past their end. (The single-file design in §2.2 is still Phase 2.) |
| 3 | ✅ Fixed | Every finished run frees its slot and calls `process_queue()`. A queue lock plus an atomic "already running" guard prevents double-starts. Downloads left in `downloading` by a crash are recovered at startup. |
| 4 | ✅ Fixed | The bridge accepts only extension origins (`chrome-extension://`, `moz-extension://`), checks `Host`, requires JSON, ignores `destination_path`, limits header and body size, and no longer sends `Access-Control-Allow-Origin: *`. `/parse-page` is extension-only, with a timeout, content-type check and 5 MB cap. *Remaining hardening:* any installed extension can still call it. A per-install token or Native Messaging would close that. |
| 5 | ✅ Fixed | A paused run no longer hashes, scans, or emits "completed". It only records progress. Progress writes can't overwrite a pause (`WHERE status='downloading'`). Pause is immediate (workers cancel mid-read) and waits for workers to stop. |
| 6 | ⏳ Phase 2 | Segment files + merge still exist. The merge is now safer: it verifies every piece, writes to `.nexmerge`, checks the size, and then renames. |
| 7 | 🟡 Partial | Hashing and AV run via `spawn_blocking`. Progress DB writes are off-thread and throttled to every 2 s. SQLite connection reuse and migrations-once are still Phase 2. |
| 8 | ⏳ Phase 2 | No retry/backoff yet. |
| 9 | 🟡 Partial | Requires 206 + a matching `Content-Range` start, rejects a `200` for partial ranges, and fails on short bodies. `If-Range`/ETag and mirror verification are still open. |
| 10 | ✅ Fixed | Every spawned process uses `CREATE_NO_WINDOW` (`process.rs`). Antivirus results are now `clean`, `threat_detected`, or `skipped` (never a false "clean"). |
| §3.4 | 🟡 Partial | The credential struct now matches the UI (`token_or_cookie`, optional username), and the `LIKE` look-alike match is fixed. Credentials are still not applied to requests and are stored in plaintext. |

Tests: 18 Rust tests pass, including regression tests for stale resume data, servers that ignore `Range`, truncated bodies, pause mid-transfer then resume (byte-exact), refused single-stream resume, bridge origin rules, and the credential/pause-race DB fixes.

### Phase 2 status (branch `feat/engine-phase-2`)

| Item | Status | What changed |
|------|--------|--------------|
| #6 Write everything twice | ✅ Fixed | One preallocated `name.nexpart` (marked **sparse on NTFS**, so the first far-offset write doesn't zero-fill gigabytes), written at the right offsets. There is no segment files and no merge pass, and finishing is an atomic rename. |
| Disk writes | ✅ Done | A dedicated writer thread gets coalesced writes (about 512 KiB or 250 ms) over a bounded channel, so network tasks never block on the disk. |
| Progress | ✅ Done | Workers update atomic counters. One ticker computes speed (smoothed) and emits events, and persists progress at most every 2 s. |
| Resume | ✅ Done | A done-range map (`name.nexpart.map`) is saved atomically and `fsync`ed before it's written. A map for a different-sized file is ignored. `If-Range` (strong ETag, else `Last-Modified`) is sent on resume; a full `200` answer means the file changed, so the engine restarts once from scratch. |
| #8 Retries | ✅ Fixed | Each range retries transient failures (connection errors, resets, early closes, 408/5xx, 429/503) with exponential backoff and jitter (1 s → 30 s cap), honouring `Retry-After`. It gives up after 6 consecutive failures with no progress. Permanent errors (404, 403, 416…) fail immediately. |
| Dynamic segmentation | ✅ Done | A connection that runs out of work steals the upper half of the biggest range still downloading. The split happens atomically under a per-range lock. |
| Connection ramp-up | ✅ Done | Up to 8 connections open immediately, and the rest ramp up gradually. A 429/503 lowers the connection cap, makes that connection hand its range back, and pauses ramp-up for 15 s. |
| Mirrors | 🟡 Improved | A mirror that errors, or reports a different file size, falls back to the original URL. The range engine now uses mirrors per range. |
| Shared HTTP client | ✅ Done | `ClientPool`, keyed by proxy config. System-proxy clients are rebuilt every 30 s so VPN/proxy toggles are picked up. Previously each download built its own client and read the registry. |
| Double probe | ✅ Fixed | Probe results are cached for 60 s, so adding a link the dialog just probed costs no second round trip. Probes honour the configured proxy. |
| Proxy setting | ✅ Fixed | Tasks are saved with the app's proxy setting when the request doesn't specify one. Before, the engine ignored the global proxy setting. |
| Speed limits | ✅ Done | Per-download limits now apply to segmented downloads (they used to apply to single-stream only), and the **global limit** from Settings and the status bar works, on a shared token bucket. Pause stays instant while connections wait for tokens. |
| §5.1 Database | ✅ Done | Busy timeout (writes used to fail instantly with "database is locked" and be silently dropped), schema setup once per process, transactional segment replacement, one query for all segments instead of one per download, and no duplicated row-mapping code. |
| Delete / cancel | ✅ Fixed | These wait for a stopping run to release its files, then remove the `.nexpart` file, its map, and any old `_seg_N` files. |
| Slow-connection detection | ⏳ | Stealing already routes work around slow connections, but a connection that is slow yet steady is never dropped. |
| Old in-progress downloads | ⚠️ | Downloads half-finished with the **old** engine (`.nexpart_seg_N` files) won't resume. They restart from zero. |

**Measured** (release build, loopback, 8 connections, `cargo test --release --test bench_engine -- --ignored --nocapture`):

| Scenario | Phase 1 engine | New engine |
|---|---|---|
| 384 MiB, all connections equal | ~850 MiB/s | ~890–1090 MiB/s |
| 64 MiB, one connection 8× slower than the rest (typical of real servers) | 8.05 s | **2.8–3.1 s** |

On loopback the OS file cache hides the cost of the old double write, so equal-speed throughput is similar. The gains show up on real disks (no extra full copy at the end) and above all on real networks, where per-connection speed varies and the old engine always waited for its slowest connection.

Tests: 40 Rust tests pass (24 unit and 16 integration), stable over 5 runs. Coverage includes range-map math, byte-exact download, resume from the map, ignored `Range`, 503 retries, mid-body resets, giving up, fast-fail on 404, work stealing, pause/resume, pause under a speed limit, speed limits, mirror fallback, and the non-resumable path.

### Phase 3 status (branch `feat/safety-phase-3`)

| Item | Status | What changed |
|------|--------|--------------|
| §3.2 Mark of the Web | ✅ Done | Every finished file gets a `Zone.Identifier` stream (`ZoneId=3`, plus `HostUrl` and `ReferrerUrl`). SmartScreen, Office Protected View and Explorer's warnings now apply to Nexdrop downloads. URLs are validated, so a crafted link can't inject extra lines. Verified in the real app. |
| §3.6 Filenames | ✅ Done | The sanitizer strips control, invisible and direction-override characters (the `invoice‮fdp.exe` trick), prefixes Windows reserved names (`CON`, `NUL`, `COM1`, `LPT9`, and their variants), trims trailing dots and spaces, caps length on character boundaries while keeping the extension, and still blocks `..` traversal. |
| §3.6 Name collisions | ✅ Done | The final path is **claimed atomically** (`create_new`). A finished download can never overwrite an existing file, even if two finish at once. This applies to both the range engine and the single-stream path. |
| §1.8 Expected checksum | ✅ Fixed | The hash is validated when the download is added (cleans up `sha256:`, case and `hash *file`; rejects non-hex or wrong lengths). It's stored in its own column and compared after hashing. On mismatch the file is **removed**, the task fails with a clear message, and the UI shows it. The Integrity tab shows whether the hash matched. |
| §3.4 Credentials | ✅ Done | Saved logins are now **applied**: a token with `=` becomes `Cookie`, any other token becomes `Bearer`, and username + password becomes `Basic`. They are sent **only over HTTPS** (or loopback), **only to the original host**, never to mirrors, and reqwest drops them on cross-host redirects. Passwords, tokens and per-task headers are **encrypted at rest with Windows DPAPI**. Plaintext logins from older versions are encrypted at startup. The settings UI only receives masked values. |
| §5.5 cURL import | ✅ Fixed | `Cookie`, `Authorization`, custom headers, `-b` and `-u` are now kept (engine-controlled headers like `Range` are dropped). They are stored encrypted per task, and resume works. Probes also use the user agent, referer and headers. |
| §3.7 Redirects | ✅ Fixed | An HTTPS → HTTP redirect is refused with a clear error instead of being followed silently. |
| §3.6 Disk space | ✅ Done | The engine checks free space before starting (32 MiB margin) and fails early with "Not enough free disk space: X needed, Y available". |
| Completion actions | ✅ Wired | "When all downloads finish" now works: notify, **sleep**, or **shut down**. Sleep and shutdown start a 30 s / 60 s countdown with a **Cancel** toast in the app. |
| Notifications and sound | ✅ Wired | Windows toasts on completion, failure and threat detection when the window isn't focused (`notify_on_complete`). The "play a sound" preference actually silences or plays the sound. When the window is focused, only a beep plays. |
| Scheduler | ✅ Rewritten | It works as a time **window**, not an exact-minute match, so sleeping PCs and late launches are handled. Overnight windows work, and the days apply to the opening day. Running downloads are paused when the window closes and **resume automatically** when it reopens. The scheduler speed cap is applied inside the window (combined with the global limit). The close action (sleep, shut down, or quit) runs through the cancellable countdown, and quitting is a clean `app.exit`, not `process::exit`. Unreadable times never block downloads. |
| Per-download schedule | ✅ Wired | A download scheduled for a time is queued when the time comes, including after the app was closed or the PC asleep at that moment. |
| Bridge | ✅ Extended | The extension may pass an optional `expected_checksum`. |
| Not done | ⏳ | A per-install token for the bridge, a link-reputation check, and an "unsafe extension" warning. |

**Verified end-to-end in the real app** (`python scripts/e2e_bridge.py`: 18 of 18 checks pass). It runs the built app against a local test server and queues downloads over the bridge, with the database backed up and restored. It checks: web and no-origin requests get 403, invalid hashes are rejected up front, a plaintext login is encrypted at startup, a correct hash completes with exact bytes and a recorded SHA-256, the Mark of the Web is present, a wrong hash fails and **removes** the file, a saved login is sent as a Basic header, and **6 downloads with a limit of 3 all complete** (live confirmation of the Phase 1 queue fix).

Tests: 80 Rust tests pass (60 unit and 20 integration).

About a dozen settings and features in the UI do nothing in the backend (see §6). The most important ones are **global speed limit, scheduled downloads, "when all finish → shut down", and site logins**.

---

## 1. Correctness problems in the download pipeline

### 1.1 The total-request timeout kills long downloads (Critical)
`create_http_client_with_proxy()` calls `.timeout(timeout)` (`client.rs:13`), and the engine passes `Duration::from_secs(30)` (`downloader.rs:38,61`). In reqwest, that timeout covers everything from connect until **the response body finishes**. Any segment that streams for more than 30 s errors with "operation timed out", and §1.5 then cancels the whole download.

**Fix:** Don't set a total timeout on the download client. Use:
- `.connect_timeout(Duration::from_secs(15))`
- `.read_timeout(Duration::from_secs(30))`, which is a per-read idle timeout. Alternatively, wrap each `stream.next()` in `tokio::time::timeout`.
- Keep a short total timeout only on the *probe* client.

### 1.2 Resume corrupts data (Critical)
- `update_progress()` (`repository.rs:199`) only writes `downloaded_bytes`. **Segments are not saved** during the download. They are persisted only when `save_download()` runs at the end (`manager.rs:267`).
- Workers open segment files with `.append(true)` (`downloader.rs:265-270`) and request `start_byte + seg.downloaded_bytes` (`:257`). If the DB value is behind the file on disk, the duplicate bytes get appended. This happens after a crash, after "Failed → Retry", or when a chunk was written after the coordinator stopped reading. That last case is the normal pause path: the coordinator returns immediately and drops the queued `Progress` messages.
- The block meant to catch this is literally empty (`downloader.rs:283-289`).

**Fix (short term):** On resume, use the on-disk length of each segment as the source of truth. Truncate any extra bytes, or set `downloaded = min(disk_len, total)`. Save segment progress in the same transaction as the task progress.
**Fix (proper):** Move to a single preallocated file with positional writes and a persisted completed-ranges map (§2.2). Then "what's on disk" and "what's recorded" can't drift apart.

### 1.3 Pause is reported as completion (Critical)
`run_download` returns `Ok(task)` with `status = Paused` (`downloader.rs:137,387`). In `manager.rs:250`, every `Ok` is handled as a finished download:
- `compute_checksums` runs on a final path that doesn't exist (the error is silently ignored).
- The antivirus scan runs on the missing file. If no AV exists, it records "clean".
- `download-completed` is emitted, so the UI shows a "Download complete" toast when you press Pause.

**Fix:** Return a dedicated outcome type, for example `enum Outcome { Completed(Task), Paused(Task), Cancelled }`. Run post-processing only for `Completed`.

### 1.4 The queue never advances, and interrupted downloads get stuck (Critical)
- After a download ends, the spawned task only emits `download-status-changed` (`manager.rs:281`). `process_queue()` is called only from add, resume, retry, startup, and the scheduler (`grep process_queue`). Queued items never start on their own.
- At startup (`lib.rs:43-49`), only `Queued` tasks are started. Tasks still marked `Downloading` from a crash or quit stay **`Downloading` forever** with no controller. The Pause button then fails silently, and auto-resume never touches them.

**Fix:** Call `process_queue()` at the end of every engine run. At startup, change every `Downloading` task to `Queued` (or `Paused` if auto-resume is off) before processing the queue.

### 1.5 One error fails everything (High)
`WorkerMessage::Failed` sets `is_cancelled`, marks the task failed, and returns (`downloader.rs:433-441`). There is no retry, no backoff, and no way to tell a 404 (permanent) from a reset or timeout (transient).

**Fix:** Retry per segment with exponential backoff plus jitter. A good starting point is 1 s, 2 s, 4 s … capped at 30 s, for 5–8 attempts. Fail the task only after a segment exhausts its retries, or on a permanent error (4xx other than 408/429). On `429` or `503`, *reduce* the number of connections and honour `Retry-After`.

### 1.6 Range responses aren't validated (High)
- Segment workers accept `200 OK` (`:314`). The response must be `206` with a `Content-Range` that matches the requested range. Otherwise, fall back to a single connection from byte 0.
- In single-stream mode, if the server ignores `Range:` and returns `200`, the whole file is appended to the existing partial (`:118-129`). If `resumable == false` and a partial exists, the code also appends. It should truncate.
- `If-Range: <etag or last-modified>` is never sent on resume. If the file changed on the server, the result is a mix of old and new bytes.
- The final size is never checked against `total_bytes` before the rename.
- Mirrors are assigned round-robin (`:248`) without checking that the mirror serves the *same* file (same size and ETag). A wrong mirror corrupts the output silently.

### 1.7 Races and lifecycle bugs (High)
- **Double start:** `process_queue()` reads the active count, then reads queued tasks from a fresh DB connection, then starts them. Two concurrent callers (add + resume, or batch add) can start the **same task twice**, with two engines writing the same files. Guard it with an async `Mutex`, and set the state to `Downloading` atomically in SQL (`UPDATE … WHERE status='queued'`, then check the affected row count).
- **Detached workers:** Segment workers are spawned with `tauri::async_runtime::spawn` and never joined (`:256`). After a pause, they keep running until their next chunk arrives. That could be a long time on a stalled socket, because pause is only checked between chunks. An immediate resume starts new workers that write to the *same* segment files. Keep the `JoinHandle`s, and use a `CancellationToken` with `select!` so cancellation is immediate. Mark the task paused only after the workers have finished.
- The single-stream pause check (`:135`) also runs only when a chunk arrives.
- `delete_download` with `delete_file = true` removes files while the workers may still be writing to them.

### 1.8 Expected checksum is never verified (High — safety)
`add_download` stores the user's expected hash *into the computed-hash fields* (`manager.rs:146-147`). On completion, they are overwritten by the computed hash (`:254-256`). Nothing ever compares the two, so the "Expected checksum" option in the New Download dialog does nothing.

**Fix:** Add an `expected_checksum` column, compare it after hashing, and mark the task `Failed` (or "Integrity mismatch") if it doesn't match.

---

## 2. Speed: why it's slow and what "fastest possible" looks like

### 2.1 Where the time goes today

| Bottleneck | Why it hurts | Location |
|---|---|---|
| Total-request timeout | Kills long transfers and forces restarts | `client.rs:13` |
| Separate segment files plus a merge pass | 2× disk writes; the merge is a single-threaded 64 KB copy. A 10 GB file needs about 20 GB free and sits at "100%" for a long time | `downloader.rs:250,451-479` |
| `tokio::fs::File::write_all` per network chunk | Each call hops to the blocking pool (`spawn_blocking` inside tokio). Chunks are often 8–16 KB, so that's thousands of thread hops per second per connection | `downloader.rs:143,334` |
| One mpsc message per chunk, `.await`ed on a 100-slot channel | Workers are throttled by how fast the coordinator drains the channel. The coordinator does bookkeeping per chunk | `downloader.rs:225,343` |
| Progress callback opens SQLite and runs 9 `ALTER TABLE`s + `CREATE TABLE IF NOT EXISTS` | Every 250 ms per download, synchronously, on a runtime thread | `manager.rs:227`, `repository.rs:8-20,93-101` |
| Progress event clones all segments every tick | Up to 32 segment structs serialized to JSON 4×/s per download | `downloader.rs:418` |
| Static segmentation | The download finishes only when the *slowest* connection does. Finished connections sit idle | `segment.rs` |
| New `reqwest::Client` per download and per probe | No connection or TLS session reuse; every client reads the Windows registry for proxy settings | `downloader.rs:59-67`, `manager.rs:72`, `commands/mod.rs:12`, `grabber_server.rs:225` |
| Double probe | The frontend probes, then `add_download` probes again (HEAD + possibly GET) before anything starts | `manager.rs:78` |
| Hashing on completion | SHA-256 **and** MD5 in one 64 KB loop, blocking a runtime thread for many seconds on large files | `checksum.rs:286-305`, `manager.rs:253` |

### 2.2 Target engine design

1. **One file, positional writes, no merge.**
   - Create `name.nexpart` once and `set_len(total)`. Mark it sparse on NTFS (`FSCTL_SET_SPARSE`) so the OS doesn't zero-fill the gaps.
   - Each connection writes at its own offset (`std::os::windows::fs::FileExt::seek_write` / `FileExt::write_at` on Unix).
   - When done, `fsync` and rename atomically. "Merging" becomes instant and needs no extra disk space.
2. **Dedicated disk writer, large batched writes.** Workers send `bytes::Bytes` (zero-copy) to a writer thread or `spawn_blocking` loop through a bounded channel. The writer coalesces data into 1–4 MiB writes per range. Network threads never block on disk.
3. **Atomic progress, not messages.** Each range keeps an `AtomicU64` cursor. A single 250–500 ms ticker reads the cursors, computes speed and ETA, emits **one batched event for all active downloads**, and persists progress at most once every 1–2 s in a single transaction.
4. **Dynamic segmentation (the core of IDM's speed):**
   - When a connection finishes its range, split the **largest remaining range** in half (respecting `min_chunk_size`) and assign the new half to the free connection.
   - Detect slow connections (for example below 25% of the median for 5 s), drop them, and reassign their ranges.
   - Ramp up: start with 2–4 connections and add more while total throughput keeps rising, up to the cap. Back off on 429/503 or on connection resets. This also implements `max_connections_per_server`, which is currently unused.
5. **Persisted range map.** Save a completed-ranges list (or bitmap) in the DB or in a `name.nexpart.json` sidecar after each flush. Resume reads it, so there's never a duplicate append.
6. **One shared HTTP client per proxy configuration**, cached in the manager:
   - Pool size of 32+ per host, `connect_timeout`, `read_timeout`, `tcp_keepalive`.
   - Keep HTTP/1.1 for range connections (reqwest is built without the `http2` feature, which is good). Several TCP connections beat one multiplexed HTTP/2 connection for raw throughput.
   - Consider `hickory-dns` with a cache for faster repeated resolution.
7. **Global token-bucket limiter** shared by every connection. One bucket for the global limit, one per download, and one for the scheduler window. The current `SpeedLimiter` (`speed.rs:246-278`) only applies to single-stream downloads, bursts for 500 ms and then sleeps, and is never applied globally.
8. **Reuse the probe.** Pass the probe result from the dialog to `add_download`, or cache probes by URL for about 60 s. Start the first range request *immediately* and let its `206`/`Content-Range` response confirm range support.
9. **Hash off-thread, and hash less.** Compute SHA-256 in `spawn_blocking` with a 1–4 MiB buffer. Compute MD5 only on demand. The `sha2` crate's `asm` feature or SHA-NI dispatch gives a large speed-up on modern CPUs.

**Expected result:** On a fast line, these changes remove the merge phase entirely, keep every connection busy until the last byte, and stop disk and DB stalls from throttling the network. Together, those three factors largely account for why "8 connections" today doesn't come close to 8× a single stream.

---

## 3. Safety and security

### 3.1 Browser bridge server (Critical)
`grabber_server.rs` is a hand-written HTTP server on `127.0.0.1:18888`:
- `Access-Control-Allow-Origin: *` with **no token** (`:89,206`). Any web page can `fetch('http://127.0.0.1:18888/download', {method:'POST', body: …})`. A `text/plain` body is a "simple request" with no preflight, and the server doesn't check `Content-Type`.
- `destination_path` and `filename` come from the request (`:133`). The filename is sanitized, but **the folder is not**, and `create_dir_all` creates it. This allows a drive-by write to `…\Start Menu\Programs\Startup\`, which means persistent code execution.
- `/parse-page` fetches any URL and returns the result to the calling page. That's **SSRF**: it can read your router admin page or intranet from any website.
- No limits: unbounded header reads, and `Content-Length` is trusted, so a client can make the server allocate large amounts of memory.
- Every accepted download force-focuses the window (`:151-155`).

**Fix:**
- Generate a random per-install secret. The extension obtains it through **Chrome Native Messaging** (better than a localhost HTTP server) or a one-time pairing.
- Reject any request without the token.
- Check `Origin` against `chrome-extension://<id>` and `moz-extension://…`. Drop `Allow-Origin: *`.
- Ignore `destination_path` from the bridge, or confine it to inside the default folder.
- Show a confirmation dialog for captured downloads, as IDM does.
- Use `axum`/`hyper` with body-size limits and timeouts instead of parsing HTTP by hand.

### 3.2 No Mark-of-the-Web (High)
Files that Nexdrop saves don't get the `Zone.Identifier` alternate data stream. That means Windows SmartScreen, Office Protected View, and "This file came from another computer" checks are **all bypassed** for anything downloaded through Nexdrop.

**Fix:** After the rename, write `:Zone.Identifier` with `ZoneId=3`, `ReferrerUrl`, and `HostUrl`, or use `IAttachmentExecute::Save`, which also triggers AV scanning the same way Windows does for browsers.

### 3.3 Antivirus result handling (High)
In `manager.rs:459-503`:
- Unknown exit codes are reported as **"clean"**. If no scanner is found, that's also "clean". It should be `error` or `skipped`, never "clean".
- `.output()` blocks a Tokio thread for the whole scan.
- The console window flashes on every completion (no `CREATE_NO_WINDOW`).
- The file is already in its final location and visible to the user while the scan runs.

**Fix:** Run the scan in `spawn_blocking` with `CREATE_NO_WINDOW`. Map exit codes precisely (`0` = clean, `2` = threat, anything else = error). Prefer AMSI or `IAttachmentExecute`. Scan *before* the rename to the final name, or keep the file quarantined until the scan returns.

### 3.4 Site credentials (High)
- **IPC contract mismatch:** the frontend sends `token_or_cookie` and an optional `username`. The backend struct expects `token` and a *required* `username` (`db/models.rs:169-176`). Tokens are silently dropped, and saving a token-only login fails deserialization.
- **Credentials are never used:** `get_site_credential_for_domain` is never called, so the vault doesn't authenticate anything.
- **Bad domain match:** `?1 LIKE '%' || domain` (`repository.rs:499`) matches `evilgithub.com` for a `github.com` entry. If the vault were wired up, this would leak credentials to other sites. Match the exact host or a `.domain` suffix only.
- **Plaintext storage:** passwords and tokens sit in plaintext SQLite. Use Windows Credential Manager (the `keyring` crate) or DPAPI.
- Never send credentials over `http://`, and never on a cross-host redirect.

### 3.5 Proxy and privacy leaks (Medium)
- `builder.build().unwrap_or_default()` (`client.rs:80`): if the proxy config is invalid, reqwest silently falls back to a **default client**, and an invalid custom proxy URL is silently ignored (`:35`). For VPN or proxy users, that means traffic leaks outside the tunnel without warning. Fail loudly instead.
- System proxy parsing (`client.rs:45-75`):
  - It handles only the `http=` entry. A `socks=host:port` entry becomes `http://socks=host:port`.
  - `ProxyOverride` (the bypass list) is ignored, so `localhost` and intranet traffic go through the proxy.
  - PAC/WPAD (`AutoConfigURL`) isn't supported.

### 3.6 File-system safety (Medium)
`sanitize_filename` (`probe.rs:164-180`) misses several cases:
- Windows reserved names (`CON`, `NUL`, `COM1`, `LPT1`…)
- Trailing dots and spaces
- Control characters
- The 255-char component limit
- Unicode bidi overrides (`U+202E`), which let `invoice‮fdp.exe` display as `invoiceexe.pdf`. Strip them and flag double extensions.

Other gaps:
- The destination folder isn't canonicalized or validated (see §3.1).
- There's no free-disk-space check before starting.
- `resolve_unique_path` is check-then-act. Use `create_new(true)` instead.

### 3.7 Smaller safety items (Low–Medium)
- **No HTTPS → HTTP redirect warning.** reqwest follows the downgrade silently.
- **`get_curl_command_cmd`** (`commands/mod.rs:169-177`) wraps values in `"` without escaping. A crafted filename or URL can inject shell commands when the user pastes the result.
- **`calculate_file_hash_cmd`** hashes any path the webview sends. It's acceptable for a local app, but restrict it to known download paths.
- **`extract_links_from_page`** (`grabber_server.rs:224-237`) creates a new client with **no timeout and no size limit**. `.text()` will read a 5 GB ISO into memory if someone grabs a direct file link.
- **`std::process::exit(0)`** in the scheduler (`scheduler.rs:316`) kills the app mid-write with no flush. Use `app.exit()` after pausing downloads.
- **`shutdown /s /t 60`** has no in-app cancel, and runs even if downloads are still active.

---

## 4. Torrent support plan

### 4.1 Library choice
| Option | Notes | Verdict |
|---|---|---|
| **`librqbit`** (Rust) | Pure Rust, async (tokio), magnet links + `.torrent`, DHT, session persistence, per-file selection, pause/resume. Actively maintained; it's used by its own desktop app, which is also Tauri-based. | ✅ **Recommended.** Same runtime, no C++ toolchain, easy to embed |
| libtorrent-rasterbar via `cxx` bindings | The most complete and proven (qBittorrent), but heavy C++ build and painful cross-compiling | Only if you need every advanced feature |
| `cratetorrent` and others | Immature or unmaintained | ❌ |

Check the current `librqbit` version for: rate limiting, uTP, UPnP/NAT-PMP port mapping, and protocol encryption (MSE/PE). Its feature set has been growing quickly, so confirm what's available before you design the settings around it.

### 4.2 Integration architecture
1. **Model:**
   - Add `kind: http | torrent` to `downloads`, plus `info_hash`, `magnet_uri`, `torrent_file_path`, `seed_ratio_limit`, `seed_time_limit`, `uploaded_bytes`, `peers`, and `seeds`.
   - Add a `torrent_files` table (`index`, `path`, `size`, `selected`, `priority`, `downloaded`).
2. **Engine abstraction:** add a `trait Transfer` (start / pause / resume / cancel / progress snapshot), implemented by `HttpTransfer` (the reworked engine from §2) and `TorrentTransfer`. The manager, queue, scheduler, and rate limits all work in terms of `Transfer`.
3. **One `librqbit::Session`** for the whole app, created at startup with its persistence folder under `%LOCALAPPDATA%\Nexdrop\torrents`. Map session stats into the existing `DownloadProgressEvent`. The UI's byte-map bar can show **piece completion** directly, because segments become a run-length encoding of the piece bitfield.
4. **Entry points:**
   - `magnet:` URI scheme and `.torrent` file association via `tauri-plugin-deep-link` and the bundle config.
   - The omnibox and paste detection recognise `magnet:?xt=urn:btih:`.
   - The extension intercepts magnet links and `.torrent` downloads.
   - Drag-and-drop of `.torrent` files. The window currently has `dragDropEnabled: false` for link drops, so handle file drops through Tauri's file-drop event or a small native handler.
5. **Add-torrent flow:** fetch metadata (from the magnet via DHT/peers, or by parsing the `.torrent`), then show the file tree with checkboxes, then start with only the selected files.

### 4.3 Torrent-specific safety
- **Path traversal from torrent metadata:** file paths come from the torrent and may contain `..`, absolute paths, or reserved names. Run every path through the same hardened sanitizer as §3.6, and confirm that the joined path stays inside the destination root.
- **Privacy:** your IP is visible to every peer. Offer "bind to VPN interface" (and refuse to run if it's down), proxy support for trackers and peers, and a clear warning in the UI.
- **Seeding limits:** default ratio and time limits, an upload-rate cap, and stop-seeding-on-complete as an option.
- **Ports:** pick a random high listen port, use UPnP/NAT-PMP only with user consent, and expect a Windows Firewall prompt on first run.
- Apply the same Mark-of-the-Web and AV-scan pipeline to completed torrent files.

### 4.4 Effort estimate
- Engine abstraction and DB migration: medium.
- librqbit embedding, progress mapping, persistence: medium.
- Add-torrent dialog (file picker) and magnet/`.torrent` entry points: medium.
- Seeding controls and VPN binding: small to medium.

**Do §2's engine rework first.** The `Transfer` trait falls out of it naturally.

---

## 5. Code quality and performance issues

### 5.1 Database layer
- **`open_or_create` on every call** (`repository.rs:8-20`): it opens a file handle, sets PRAGMAs, runs `CREATE TABLE IF NOT EXISTS`, and attempts 9 `ALTER TABLE`s. That happens on every IPC command and 4×/s per active download.
  **Fix:** One long-lived connection on a dedicated DB thread (or `r2d2`/`deadpool-sqlite`), cached prepared statements, and migrations run **once** at startup, versioned with `PRAGMA user_version`.
- **Migration by error suppression:** `let _ = conn.execute("ALTER TABLE …")` hides real errors too.
- **N+1 queries:** `get_all_downloads` runs one segments query per task (`:374`), and the frontend calls it on *every* status-change event. Use a single join, or load segments only for active or expanded tasks.
- **Duplicated code:** the row → `DownloadTask` mapping is copy-pasted between `get_all_downloads` and `get_download` (`:315-369` and `:393-447`). Extract a `fn map_row`.
- **Writes aren't transactional:** `save_download` + `save_segments` should run in one transaction.
- **Settings stored as one JSON blob:** fine, but `load_settings` *writes* defaults as a side effect of reading.

### 5.2 Async hygiene
- No `spawn_blocking` anywhere. Blocking work that runs on Tokio worker threads today:
  - rusqlite calls (all of them)
  - `compute_checksums` (`manager.rs:253`, `commands/mod.rs:145,224`)
  - `MpCmdRun` via `.output()` (`manager.rs:475`)
  - `std::fs::remove_file` and `create_dir_all` in commands
- Tauri `async` commands that do synchronous work occupy runtime threads. Either make those commands sync (Tauri runs them off the main thread) or wrap the work in `spawn_blocking`.
- `parking_lot::Mutex` around `active_controllers` is fine, but `process_queue` needs an **async** mutex around its whole body (see §1.7).

### 5.3 Error handling
- Errors are `String` everywhere, and `thiserror` is a dependency that's **never used**. Introduce `enum NexdropError { Http, Io, Db, Range, Cancelled, … }`. You need it to tell transient from permanent errors for retries.
- There are 27 non-test `unwrap()`s, for example `final_dest.parent().unwrap()` and `.to_str().unwrap()` in `downloader.rs:186,452`, which panic on non-UTF-8 Windows filenames.
- `batch_add_downloads_cmd` (`commands/mod.rs:41-48`) logs and swallows per-item failures, then returns `Ok`. The UI can't tell which links failed. It's also **sequential**: 50 links means 50 probes back-to-back, plus a full queue scan each time. Probe them concurrently (bounded, about 6 at once) and return per-item results.

### 5.4 Probe
- **HEAD isn't proof of range support:** a successful HEAD trusts `Accept-Ranges` (`probe.rs:22-32,82-89`). Many CDNs advertise it and then return `200`. Confirm with the first real range request (see §2.2.8).
- **Request headers are incomplete:** the probe doesn't send the user's User-Agent, Referer, cookies, or credentials. Sites that require them fail the probe even though the download itself would work.
- **Filename decoding bug:** `urlencoding_decode` uses `form_urlencoded::parse` (`:157-162`), which treats `&` and `=` as separators and `+` as a space. A file named `a&b+c.zip` becomes `ab c.zip`. Use `percent_encoding::percent_decode_str`.

### 5.5 cURL import
`parse_curl_command_cmd` (`commands/mod.rs:243-319`) keeps only UA, Referer, `-o`, and `-x`.
- It **drops `Cookie`, `Authorization`, and every other header**. Those are exactly what "Copy as cURL" is for, so most authenticated downloads imported this way fail.
- `AddDownloadRequest.custom_headers` exists in the model but the engine never uses it. Store headers per task and send them on every range request.
- The tokenizer handles bash quoting only. Chrome's "Copy as cURL (cmd)" format, with `^` escapes, isn't parsed.

### 5.6 Scheduler (`scheduler.rs`)
- **Exact-minute matching** (`:231,259,273`): if the PC is asleep at 02:00, or the app starts at 02:05, the window is missed entirely. Evaluate "are we inside the window?" instead of "is it exactly the start minute?".
- **Paused downloads never come back:** at the stop time, active downloads are *paused* (`:284-287`). The next start only processes `Queued` tasks, so downloads paused by the scheduler never resume. Move them back to `Queued`, or track "paused by scheduler" separately.
- **Doesn't gate the queue:** with the scheduler enabled, newly added downloads still start immediately, outside the window.
- **`scheduler_speed_limit` is never applied.**
- **Console flashes and a hard exit:** commands are spawned without `CREATE_NO_WINDOW`, and the code uses `process::exit` (see §3.7).

### 5.7 Browser extension
- **No cookies are sent** with captured downloads. IDM forwards the browser's cookies. Use `chrome.cookies.getAll({url})` (needs the `cookies` permission) and send them through the authenticated bridge, along with the browser's real User-Agent.
- **"Grab all links on page" does nothing:** it POSTs to `/parse-page` and **ignores the response**, so nothing appears in the app. It also re-fetches the page server-side without cookies, so logged-in pages return nothing useful. Extract the links in the content script from the live DOM and send them to the app's Add-multiple flow.
- **`<all_urls>` content script on every page:** it's heavy for a video-overlay feature. Inject on demand (`activeTab` + `scripting`) or limit it to pages that contain media.
- **Intercept logic:** `downloadItem.fileSize > 5 MB` is usually `-1`/`0` at `onCreated`, so size-based capture rarely triggers. Use `onDeterminingFilename`, or check `totalBytes` once known.

### 5.8 Tests
- **The pause/resume test passes without testing anything** (`tests/download_integration_test.rs:198-290`). The 1 MB local transfer finishes before the 20 ms pause fires. Then `if let Ok(paused) … if paused.status == Paused { … }` skips every assertion.
- **Missing tests:**
  - resume after a crash with segment files on disk
  - a server that ignores `Range` (returns 200)
  - a mid-stream connection reset (retry)
  - a download longer than 30 s (would catch §1.1)
  - an ETag change between pause and resume
  - concurrent `process_queue`
  - the bridge rejecting a request without a token
- Add a throttled mock server (for example, bytes trickled with sleeps) so these tests are deterministic.

---

## 6. Settings and features with no backend effect

| Setting / feature | Status |
|---|---|
| `global_speed_limit` | Never read |
| `scheduler_speed_limit` | Never read |
| `max_connections_per_server` | Never read |
| `on_complete_action` (notify / sleep / shutdown when all finish) | Never read |
| `notify_on_complete`, `sound_enabled` | Never read. There's no notification plugin |
| Per-download `scheduled_time` | Tasks are saved as `Scheduled`, but nothing ever starts them |
| Site logins vault | Stored, never applied (and the field names don't match the frontend; see §3.4) |
| Expected checksum | Stored in the wrong field and never compared (§1.8) |
| `custom_headers` | In the request model, never used |
| Mirrors | Used, but without verifying that they serve the same file (§1.6) |
| "Grab all links" from the extension | Response ignored (§5.7) |
| README: "exponential backoff" | Not implemented (§1.5) |

---

## 7. Recommended roadmap

**Phase 1 — Stop data loss and stop the bleeding (small, do now)**
1. Replace the total timeout with connect and read timeouts (§1.1).
2. Call `process_queue()` after each run, and recover stuck `Downloading` tasks at startup (§1.4).
3. Return a proper outcome enum; pause ≠ complete (§1.3).
4. Resume from the on-disk segment length; require `206` + a matching `Content-Range` (§1.2, §1.6).
5. Lock down the bridge: token + Origin check, no `destination_path`, no `/parse-page` SSRF (§3.1).
6. `CREATE_NO_WINDOW` on every spawned process; run AV and hashing in `spawn_blocking` (§3.3).
7. Fix the credential field mismatch and the `LIKE` domain match (§3.4).

**Phase 2 — Engine rewrite for speed (medium)**
- Single preallocated file + positional writes + batched disk writer + atomic progress (§2.2.1–3)
- Per-segment retry with backoff; dynamic segmentation, work stealing, connection ramp-up (§1.5, §2.2.4)
- Persisted range map; `If-Range`; final size check (§2.2.5, §1.6)
- Shared client cache; global token-bucket limiter; reuse the probe (§2.2.6–8)
- One long-lived DB connection, versioned migrations, transactional batched progress writes (§5.1)

**Phase 3 — Safety polish (small to medium)**
- Mark-of-the-Web, a hardened filename sanitizer, a disk-space check, a redirect downgrade warning (§3.2, §3.6, §3.7)
- Credentials in Windows Credential Manager, actually applied to requests (§3.4)
- Expected-checksum verification; correct AV exit-code handling (§1.8, §3.3)
- Wire up the dead settings: speed limits, completion actions, per-download schedules, notifications (§6)

**Phase 4 — Torrents (medium to large)**
- `Transfer` trait → embed `librqbit` → magnet/`.torrent` entry points → file-selection dialog → seeding and VPN-binding controls (§4)

**Phase 5 — Extension and tests**
- Native Messaging + cookie forwarding + DOM-based link grabbing (§5.7)
- The missing tests listed in §5.8, run in CI with `cargo test --all-targets`
