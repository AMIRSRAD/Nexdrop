"""Repairs finished downloads whose bytes ended up in the wrong place.

    python scripts/repair_parts.py "<folder>" "<name contains>"

For every completed download in Nexdrop's database whose file name contains the text and
that lives in the folder, this compares samples of the file with what the server serves,
works out which stretches differ, downloads exactly those stretches again from the original
link and writes them in place, then checks the file again. Only bytes that the server just
served are ever written. Files that already match are left alone (not even opened for writing).
"""
import os, shutil, sqlite3, sys, tempfile, threading, time, urllib.request
from concurrent.futures import ThreadPoolExecutor

MB = 1 << 20
SAMPLE = 16 * 1024
STEP = 2 * MB
UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) Nexdrop/1.0'


def fetch(url, start, end, timeout=60):
    """Bytes start..end inclusive; None when the server cannot answer reliably."""
    for attempt in range(4):
        try:
            req = urllib.request.Request(url, headers={'Range': f'bytes={start}-{end}', 'User-Agent': UA})
            with urllib.request.urlopen(req, timeout=timeout) as r:
                cr = r.headers.get('Content-Range', '')
                if r.status != 206 or not cr.startswith(f'bytes {start}-'):
                    return None
                body = r.read()
                if len(body) == end - start + 1:
                    return body
        except Exception:
            time.sleep(1 + attempt)
    return None


def sample_offsets(total, shift=0):
    offs = [k * STEP + (k % 7) * 4099 + STEP // 2 + shift for k in range(total // STEP + 1)]
    offs = [o for o in offs if o + SAMPLE <= total]
    if not offs or offs[-1] != total - SAMPLE:
        offs.append(total - SAMPLE)
    return offs


def scan(path, url, total, shift=0, pool=None):
    offs = sample_offsets(total, shift)
    f = open(path, 'rb')
    lock = threading.Lock()

    def check(o):
        theirs = fetch(url, o, o + SAMPLE - 1, 30)
        if theirs is None:
            return o, None
        with lock:
            f.seek(o)
            mine = f.read(SAMPLE)
        return o, mine == theirs

    with ThreadPoolExecutor(12) as ex:
        res = sorted(ex.map(check, offs))
    f.close()
    unknown = sum(1 for _, v in res if v is None)
    bad = []
    for i, (o, v) in enumerate(res):
        if v is False:
            lo = res[i - 1][0] + SAMPLE if i else 0
            hi = res[i + 1][0] + SAMPLE if i + 1 < len(res) else total
            bad.append((lo, min(hi, total)))
    merged = []
    for lo, hi in sorted(bad):
        if merged and lo <= merged[-1][1]:
            merged[-1] = (merged[-1][0], max(merged[-1][1], hi))
        else:
            merged.append((lo, hi))
    return merged, unknown, len(res)


def repair_range(path, url, lo, hi):
    """Downloads lo..hi (exclusive end) in pieces and writes it into the file."""
    PIECE = 8 * MB
    pieces = [(s, min(s + PIECE, hi)) for s in range(lo, hi, PIECE)]
    out = open(path, 'r+b')
    lock = threading.Lock()
    failed = []

    def one(p):
        s, e = p
        body = fetch(url, s, e - 1, 120)
        if body is None:
            failed.append(p)
            return
        with lock:
            out.seek(s)
            out.write(body)

    with ThreadPoolExecutor(4) as ex:
        list(ex.map(one, pieces))
    out.flush()
    os.fsync(out.fileno())
    out.close()
    return failed


def main():
    folder, needle = sys.argv[1], sys.argv[2]
    src = os.path.join(os.environ['LOCALAPPDATA'], 'Nexdrop', 'nexdrop.db')
    copy = os.path.join(tempfile.gettempdir(), 'repair_copy.db')
    for e in ('', '-wal', '-shm'):
        if os.path.exists(src + e):
            shutil.copy2(src + e, copy + e)
    db = sqlite3.connect(copy)
    rows = db.execute("select filename,url,total_bytes,destination_path from downloads where status='completed' and filename like ? order by filename", (f'%{needle}%',)).fetchall()
    folder = os.path.normcase(os.path.normpath(folder))
    summary = []
    for name, url, total, dest in rows:
        path = os.path.join(dest, name)
        if os.path.normcase(os.path.normpath(dest)) != folder or not os.path.exists(path) or os.path.getsize(path) != total:
            continue
        t0 = time.time()
        bad, unknown, n = scan(path, url, total)
        status = 'ok'
        fixed = 0
        if bad:
            fixed = sum(h - l for l, h in bad)
            print(f'{name}: {len(bad)} damaged stretches ({fixed / MB:.0f} MiB) -> repairing', flush=True)
            for lo, hi in bad:
                failed = repair_range(path, url, lo, hi)
                if failed:
                    print('   could not re-fetch', failed, flush=True)
            bad2, unknown2, _ = scan(path, url, total, shift=777777)
            status = 'repaired' if not bad2 else f'STILL DAMAGED {bad2[:3]}'
        print(f'{name[-14:]}: {status}  samples {n}, unanswered {unknown}, {time.time() - t0:.0f}s', flush=True)
        summary.append((name, status, fixed))
    print('DONE', sum(1 for s in summary if s[1] == 'repaired'), 'repaired,', sum(1 for s in summary if s[1] == 'ok'), 'already fine,', sum(1 for s in summary if s[1].startswith('STILL')), 'still damaged')


if __name__ == '__main__':
    main()
