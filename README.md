# Nexdrop — A Modern Internet Download Manager

> **"Everything users need, nothing they don't."**

**Nexdrop** is a free, high-performance, cross-platform Internet Download Manager crafted with the visual elegance and responsiveness of modern desktop apps (Linear, Arc, Raycast) combined with the power of a multi-threaded Rust download acceleration engine.

---

## Key Highlights & Capabilities

- **Multi-Part Accelerated Downloads:**
  - Automatic detection of HTTP `Accept-Ranges` and `Content-Range`.
  - Splits files into $N$ contiguous segment streams (1 to 32 connections).
  - High-speed concurrent disk streaming and atomic segment stitching.
  - Graceful fallback to single-stream download for non-range servers.
- **Resilience & Fault Recovery:**
  - Pause and resume downloads across application restarts.
  - Persisted segment byte ranges and progress in local SQLite database.
  - Automatic retry with exponential backoff on network dropouts.
- **Queue & Bandwidth Management:**
  - Concurrency management (`max_active_downloads`).
  - Priority queuing (Urgent, High, Normal).
  - Configurable global and per-download speed caps.
  - Scheduled downloads support.
- **Refined Modern Desktop UI/UX:**
  - Dark glassmorphism interface with Tailwind CSS v4 and Framer Motion.
  - Live multi-stream segment visualizer showing individual worker activity.
  - URL probing before download start (detects size, server range support, file type, and category).
  - Clipboard detection and batch URL importing.
  - Quick action controls: Pause, Resume, Retry, Cancel, Open File, and Reveal in File Explorer.

---

## Architecture Overview

```
Nexdrop
├── src-tauri/               # Rust Desktop Core (Tauri 2)
│   ├── src/
│   │   ├── db/              # SQLite Persistence (rusqlite bundled)
│   │   │   ├── models.rs    # Core domain models (DownloadTask, Segment, Settings)
│   │   │   └── repository.rs# SQLite CRUD operations & queries
│   │   ├── engine/          # Download Acceleration Engine
│   │   │   ├── probe.rs     # HTTP HEAD / Range=0-0 probing & filename extraction
│   │   │   ├── segment.rs   # Range partitioning & segment math
│   │   │   ├── downloader.rs# Multi-stream worker engine & single-stream fallback
│   │   │   ├── speed.rs     # Rolling window speed calculator & rate limiter
│   │   │   └── manager.rs   # Queue orchestrator & Tauri event emitter
│   │   ├── commands/        # Tauri IPC commands exposed to webview
│   │   ├── lib.rs           # Application setup & handler registration
│   │   └── main.rs          # Application entrypoint
│   └── tests/               # Automated Integration & Unit Tests
└── src/                     # React 19 + TypeScript + Tailwind CSS Frontend
    ├── components/
    │   ├── layout/          # Sidebar, Header & navigation
    │   ├── downloads/       # DownloadCard, DownloadRow, SegmentBar, FileIcon, EmptyState
    │   └── modals/          # NewDownloadModal, BatchDownloadModal, SettingsModal
    ├── services/api.ts      # Tauri IPC client & real-time event listeners
    └── types/download.ts    # TypeScript interface definitions
```

---

## Getting Started

### Prerequisites

- **Node.js**: v20+ or v24+
- **pnpm**: 11+
- **Rust**: 1.85+ (Edition 2024 / rustc 1.90+)
- **Windows**: WebView2 runtime (pre-installed on Windows 10/11)

### Development Setup

1. **Install Dependencies:**
   ```bash
   pnpm install
   ```

2. **Run Frontend in Dev Server:**
   ```bash
   pnpm dev
   ```

3. **Run Full Desktop App (Tauri 2):**
   ```bash
   pnpm tauri:dev
   ```

---

## Testing & Verification

Nexdrop includes unit and integration tests covering SQLite migrations, segment partitioning math, and an automated end-to-end integration test with an in-process mock HTTP range server:

```bash
# Run all tests in the Rust engine
pnpm test:backend
```

Tests verify:
- Accurate segment chunk splitting with zero overlaps and zero gaps.
- SQLite database migrations, updates, and status transitions.
- Multi-stream range request execution and byte-for-byte file verification.
- Pause/resume state recovery.

---

## Production Build & Packaging

Build the desktop installer:

```bash
# Build frontend and production desktop binary
pnpm tauri:build
```

Generated installer packages will be placed in `src-tauri/target/release/bundle/`.
