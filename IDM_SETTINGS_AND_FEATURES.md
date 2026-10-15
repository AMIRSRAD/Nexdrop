# Complete Guide to IDM Settings & Features and Nexdrop Implementation Plan

This document provides a comprehensive analysis of **Internet Download Manager (IDM)**'s settings, tabs, architecture, and feature set, alongside the concrete implementation plan for **Nexdrop**.

---

## 1. Complete Breakdown of IDM Dialogs & Tabs

### Tab 1: General
- **Browser Integration:** Hooking into Chrome, Firefox, Edge, Opera, Brave, Vivaldi via native messaging extension.
- **Keys for forced downloading & prevention:** Holding `Alt` or `Shift` while clicking a link to force or prevent IDM from catching the link.
- **Clipboard Monitoring:** Auto-detecting file URLs copied to Windows clipboard.
- **URL Sniffer / Media Grabber:** Overlay floating video/audio download panel above HTML5 video players (`.mp4`, `.webm`, `.ts`, `.m3u8`).
- **Start IDM on Windows Startup:** Registry run key integration.

### Tab 2: File Types
- **Automatically captured extensions:** Comma-separated list (e.g. `ZIP, RAR, 7Z, ISO, EXE, MSI, TAR, GZ, MP4, MKV, MP3, PDF, APK, DMG, DOCX, XLSX`).
- **Ignore / Exception List:** Domains where IDM will never automatically intercept downloads (e.g. intranet sites, specific web apps).

### Tab 3: Connection
- **Connection Type / Speed:** Presets (Dial-Up, Low Speed, Medium Speed, High Speed / Direct Fiber).
- **Max Connection Number:** Dropdown selector (1, 2, 4, 8, 16, 24, 32 connections per download).
- **Minimum Segment / Chunk Size:** Minimum byte threshold before splitting a file into another segment (prevents excessive connections on small files).
- **Max Connections per Server:** Enforces a limit on simultaneous streams to a single domain to avoid HTTP 429 Too Many Requests or CDN bans.
- **Bandwidth / Speed Limiter:** Global toggle with configurable max throughput in KB/s.

### Tab 4: Save To (Storage & Categories)
- **Default Download Directory:** Base directory for downloads.
- **Smart Category Subfolders:**
  - Compressed: `Downloads/Compressed/`
  - Documents: `Downloads/Documents/`
  - Music: `Downloads/Music/`
  - Video: `Downloads/Videos/`
  - Programs: `Downloads/Programs/`
- **Temporary Directory:** Dedicated folder for partial `.part` / `.nexpart` segment pieces before assembly.
- **Remember last folder per category:** Remembers custom folder selection for specific types.

### Tab 5: Downloads (Dialogs & Automation)
- **Start download immediately without prompt:** 1-click silent queueing.
- **Show download progress dialog:** Floating progress box with segment bars.
- **Show download complete dialog:** Prompt to Open File, Open Folder, or Close.
- **Virus Scanner Integration:** Auto-triggering Windows Defender or antivirus scanner upon file completion.

### Tab 6: Proxy / Socks
- **Proxy Modes:**
  - No Proxy (Direct)
  - Use System / IE Proxy settings (VPN / Windows network proxy)
  - Manual Proxy Configuration:
    - HTTP Proxy (Host, Port, User, Pass)
    - HTTPS Proxy (Host, Port, User, Pass)
    - FTP Proxy (Host, Port, User, Pass)
    - SOCKS v4 / SOCKS v5 Proxy (Host, Port, User, Pass)
  - PAC (Proxy Auto-Configuration) script URL
- **Bypass List:** Semicolon-delimited domains/IPs to bypass (`localhost; 127.0.0.1; *.local; 192.168.*`).
- **Per-download Proxy Override:** Selecting a dedicated proxy or direct connection for an individual URL.

### Tab 7: Site Logins (Credentials Manager)
- **Saved Authentication:** Domain/URL prefixes with associated Username and Password for HTTP Basic, Digest, or NTLM authentication.
- **Bearer Tokens & Custom Headers:** Sending authentication tokens or cookies with specific requests.

### Tab 8: Dial-Up / VPN Networking
- **Dial-Up & VPN connection selection:** Automatically dial VPN/PPPoE before scheduled downloads and disconnect when completed.
- **Hang up / Disconnect when done:** Disconnects internet after queue completion.

### Tab 9: Scheduler (Queues & Schedules)
- **Multiple Queues:** Main Download Queue, Synchronization Queue, and Custom Named Queues.
- **Start Time:** One-time (specific date & time) or Periodic (selected days of the week: Mon, Tue, Wed, Thu, Fri, Sat, Sun).
- **Stop Time:** Hard cutoff time to stop downloading (e.g. 07:00 AM) to preserve peak hours.
- **Number of Retries on Connection Failure:** Exponential backoff retry counter.
- **Post-Schedule Actions:**
  - Turn off computer
  - Put computer into sleep/hibernate mode
  - Exit download manager
  - Disconnect modem / VPN
- **Scheduled Speed Limiter:** Switch to quiet speed limit mode during specific hours.

---

## 2. Advanced Modern Capabilities Beyond Traditional IDM

1. **Multi-Source / Mirror Downloads:** Download segments from multiple mirror URLs for the same file simultaneously.
2. **Chunk Size Customization:** Configure chunk boundaries (e.g., minimum chunk size 512KB, 1MB, 2MB, 5MB).
3. **Cryptographic Checksum Verification:** Instant SHA-256 and MD5 hashing on completion with 1-click verification.
4. **Live Stream Bandwidth Inspector:** Interactive graphical view of active segments, speeds, and ETA.
5. **Pattern Sequence Batch Downloader:** Generate hundreds of URLs using `[01-24]` bracket sequence syntax.
6. **Export as cURL / JSON:** 1-click conversion to CLI curl command or full backup.

---

## 3. Nexdrop Implementation Roadmap

| Feature Group | Setting / Feature | Nexdrop Architecture Location |
| :--- | :--- | :--- |
| **Connection & Chunks** | Min Chunk Size Selector (256KB – 10MB) | `src-tauri/src/engine/segment.rs` & `models.rs` |
| | Max Connections Per Download (1 – 32) | `models.rs` & `downloader.rs` |
| | Max Concurrent Active Downloads (1 – 20) | `manager.rs` queue processor |
| | Max Connections Per Domain | `downloader.rs` host connection pool |
| **Multi-Source / Mirrors** | Mirror URL Fallback & Concurrent Multi-Source | `probe.rs`, `downloader.rs`, `AddDownloadRequest` |
| **Scheduler** | Full Scheduler Modal & Background Worker | `src-tauri/src/engine/scheduler.rs` & React UI |
| | Days of Week + Start Time + Stop Time | SQLite `schedules` table & cron loop |
| | Post-Schedule Actions (Sleep, Shutdown, Exit) | Windows power commands & Tauri exit |
| **Proxy & VPN** | Direct / System VPN / Custom HTTP & SOCKS5 | `client.rs` & Settings modal |
| | Per-Download Proxy Override | `AddDownloadRequest` & `DownloadTask` |
| | Proxy Bypass List | `client.rs` WinINet & WebView2 args |
| **Credentials & Headers** | Site Logins & Custom Headers / Cookies | `src-tauri/src/db/repository.rs` site logins |
| **Speed Limiter** | Global Speed Limiter + Scheduled Limiter | `src-tauri/src/engine/speed.rs` rate limiter |
