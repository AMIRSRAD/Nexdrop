"""Starts a local BitTorrent swarm for manually testing Nexdrop's torrent support.

    cargo build --manifest-path src-tauri/Cargo.toml --example seed_torrent
    python scripts/torrent_swarm.py

It creates a small multi-file torrent, runs a tiny HTTP tracker and a real
seeder on 127.0.0.1, writes the .torrent file, and prints a magnet link that
points at the tracker. Paste the magnet link (or open the .torrent file) in
Nexdrop. Ctrl+C stops everything and removes the temporary files.
"""
import http.server, os, shutil, struct, subprocess, sys, tempfile, threading, time, urllib.parse

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SEEDER = os.path.join(ROOT, 'src-tauri', 'target', 'debug', 'examples', 'seed_torrent.exe' if os.name == 'nt' else 'seed_torrent')
TRACKER_PORT = 8766
SEED_PORT = 39517


def make_content(base):
    folder = os.path.join(base, 'Nexdrop Swarm Test')
    os.makedirs(os.path.join(folder, 'extras'), exist_ok=True)

    def blob(n, seed):
        return bytes((i * 31 + seed * 7 + 5) % 251 for i in range(n))

    files = {
        'movie.bin': blob(24_000_000, 1),
        os.path.join('extras', 'commentary.bin'): blob(8_000_000, 2),
        'cover.bin': blob(500_000, 3),
    }
    for rel, data in files.items():
        with open(os.path.join(folder, rel), 'wb') as f:
            f.write(data)
    return folder, files


class Tracker(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_GET(self):
        # One peer: the local seeder.
        peers = bytes([127, 0, 0, 1]) + struct.pack('>H', SEED_PORT)
        body = b'd8:completei1e10:incompletei0e8:intervali30e5:peers6:' + peers + b'e'
        self.send_response(200)
        self.send_header('Content-Type', 'text/plain')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main():
    if not os.path.exists(SEEDER):
        print('Build the seeder first: cargo build --manifest-path src-tauri/Cargo.toml --example seed_torrent')
        return 2
    base = tempfile.mkdtemp(prefix='nexdrop_swarm_')
    folder, files = make_content(base)
    torrent = os.path.join(base, 'swarm.torrent')

    server = http.server.ThreadingHTTPServer(('127.0.0.1', TRACKER_PORT), Tracker)
    threading.Thread(target=server.serve_forever, daemon=True).start()

    proc = subprocess.Popen(
        [SEEDER, folder, f'http://127.0.0.1:{TRACKER_PORT}/announce', str(SEED_PORT), torrent],
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
    )
    magnet = None
    for line in proc.stdout:
        line = line.strip()
        if line.startswith('MAGNET '):
            magnet = line[7:]
        if line == 'READY':
            break
    print('MAGNET', magnet)
    print('TORRENT', torrent)
    print('FOLDER ', folder)
    print('FILES  ', {k: len(v) for k, v in files.items()})
    sys.stdout.flush()
    try:
        while proc.poll() is None:
            time.sleep(1)
    except KeyboardInterrupt:
        pass
    finally:
        proc.terminate()
        server.shutdown()
        shutil.rmtree(base, ignore_errors=True)


if __name__ == '__main__':
    sys.exit(main())
