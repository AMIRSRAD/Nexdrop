# Download engine: speed and reliability

Nexdrop's HTTP engine was measured against a local **network lab**
(`src-tauri/tests/network_lab.rs`). The lab is an HTTP/1.1 server that can:

- cap each connection's speed and add latency
- cut responses off and return 503s
- stall a connection or go offline completely
- ignore Range requests or hide range support from HEAD
- redirect, and hand out signed links that expire

Every scenario checks the downloaded file byte for byte.

```
cargo test --release --test network_lab -- --ignored --nocapture --test-threads=1
```

## Results

| Scenario | Before | After |
|---|---|---|
| 16 connections, each capped at 2 MiB/s | 16.9 MiB/s (ramp-up too slow) | **31.0 MiB/s** |
| 8 connections, each capped at 2 MiB/s | 15.6 MiB/s | 15.6 MiB/s (at the cap) |
| Server supports ranges but HEAD doesn't say so | 2.0 MiB/s, 1 connection | **16.0 MiB/s**, 8 connections |
| Server advertises ranges but ignores them | **fails**: "set connections to 1" | completes (falls back to one stream) |
| No range support + connection drops | **fails** on the first drop | completes (restarts, up to 5 attempts) |
| Network down for 90 s mid-download | **fails** after ~60 s | completes about 8 s after the network returns |
| One connection stalls forever | 31.7 s (waits out a 30 s timeout) | **7.8 s** |
| Redirected link (e.g. GitHub releases, CDNs) | redirect followed on every range (9 hops) | followed once (1 hop) |
| Signed link expires after 3 uses | worked, but only because of the extra redirects above | refreshed through the original link automatically |
| Flaky: 40% of responses cut, 15% busy | n/a (harsher than the old lab) | completes, byte-exact |
| Loopback, uncapped (engine overhead) | 947 MiB/s | 1,044–1,234 MiB/s |

On real links, more connections only help when each connection is limited,
for example by the server, a CDN, or a long distance. The scaling rows show
the engine reaches the cap of every connection it opens.

## What changed

**Speed**
- **Range support is checked with a real 1-byte request** whenever the HEAD
  reply doesn't prove it. Many servers support ranges without saying
  `Accept-Ranges`; those downloads previously used one connection.
- **Faster ramp-up**: up to 16 connections at once, then 4 more every 250 ms,
  up from 8 then 2 every 500 ms. Throttling detection still backs off on
  429/503.
- **Redirects are resolved once.** Range requests go straight to the final
  address. Cookies and logins are only sent to it when it's the same host as
  the original link.
- Single-stream downloads write in 1 MiB buffered blocks instead of one disk
  call per packet.

**Reliability**
- **Network outages are waited out for up to 10 minutes.** Requests that get
  no answer at all retry every 1–8 s without using up the normal retry budget,
  so the download resumes within seconds of the connection returning.
- **Stalled connections are replaced.** A connection is dropped after 6 s of
  silence if other connections received data in the meantime, or after 15 s
  if all of them went quiet.
- **Servers that ignore ranges** switch to a single stream automatically. The
  task is then marked as not resumable.
- **Single-stream downloads retry** dropped connections and server errors, up
  to 5 attempts.
- **Expired signed links** are refreshed through the original link.
  - 401, 403, 404 or 410 from the resolved address triggers a refresh.
  - The download gives up with a clear message if the link keeps expiring.
- **The probe retries** transient failures twice before giving up.
- **A 404 or 403 at probe time now fails with that status.** Previously the
  error page itself could be treated as the file.
- **Unrecoverable responses fail fast.**
  - A refused HTTPS→HTTP redirect.
  - 416 (the file got shorter), handled as "file changed": the download
    restarts once from scratch.

## Already in place (unchanged)

- One preallocated sparse file per download, written by a dedicated disk thread.
- A sidecar map for exact resume.
- Work stealing so no connection idles while another still has work.
- `If-Range` validation on resume, and mirror fallback.
- Exponential backoff with `Retry-After`.
- Shared HTTP clients (connection, TLS and DNS reuse).
- HTTP/1.1 only, so each "connection" really is a separate TCP connection.
  HTTP/2 would multiplex them all onto one.

## Not done / next candidates

- `max_connections_per_server` is still a setting that nothing reads.
- No automatic re-queue after a download fails for good, for example after
  10 minutes offline. The user retries manually.
- No duplicate-request "endgame" for the last slow range. Work stealing plus
  the stall rule cover the measured cases.

## Streaming video (HLS and DASH)

Links ending in `.m3u8` or `.mpd` (or served with an HLS/DASH content type)
are downloaded as streams: `src-tauri/src/engine/stream/`.

- **Qualities:** the playlist is read first; the Add dialog shows a Quality
  picker (best first). Choosing nothing downloads the best one.
- **Download:** segments are fetched 16 at a time, retried like any request,
  decrypted if AES-128 protected, and appended in order. Progress is saved
  every two seconds, so pausing, quitting or crashing resumes from the last
  whole segment, and the playlist is read fresh on each start so signed links
  stay valid.
- **One playable file:** transport streams become one `.ts`. When video and
  audio are separate fragmented-MP4 tracks (the usual case) they are joined
  into one `.mp4` by interleaving the fragments, with no re-encoding.
- **Verified against real streams** (Apple's HLS fMP4 sample with separate
  audio, Akamai's Big Buck Bunny over DASH): the joined files decode
  completely in ffmpeg (`scripts/verify_streams.py`).
- **Refused with a reason:** live streams, DRM, and SAMPLE-AES.
- **Tests:** `tests/stream_test.rs` (10 scenarios), `scripts/e2e_streams.py`
  (the real app, 15 checks), `tests/stream_real.rs` (real internet streams).
