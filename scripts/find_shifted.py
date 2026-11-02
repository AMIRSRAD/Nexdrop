"""Finds (and with --repair, fixes) stretches written from a CDN's wrong-slice answers.

    python scripts/find_shifted.py "<folder>" "<name contains>" [--repair]

The CDN bug answered a range that started at S (inside a 4 MiB slice) with the data
that really starts at the next slice boundary B. So the damaged stretch at S holds a
copy of the bytes found at B. For every boundary B this looks for B's first 4 KiB
earlier in the same file, inside (B - 4 MiB, B); compressed data never repeats by
chance, so a hit at S means a shifted stretch starting at S. No network needed to find
it; --repair fetches exactly those stretches again from the original link.
"""
import mmap, os, sys, sqlite3, shutil, tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from repair_parts import repair_range, fetch

MB = 1 << 20
SLICE = 4 * MB
PROBE = 4096


def find_runs(path):
    with open(path, 'rb') as f:
        m = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
        total = len(m)
        runs = []
        for b in range(SLICE, total - PROBE, SLICE):
            probe = m[b:b + PROBE]
            s = m.find(probe, b - SLICE + 1, b)
            if s < 0:
                continue
            # length of the copy: compare m[s:] with m[b:] in 64 KiB steps, then bytewise
            n = 0
            step = 64 * 1024
            while b + n + step <= total and m[s + n:s + n + step] == m[b + n:b + n + step]:
                n += step
            while b + n < total and m[s + n] == m[b + n]:
                n += 1
            runs.append((s, s + n, b - s))
        m.close()
    return runs, total


def main():
    folder, needle, repair = sys.argv[1], sys.argv[2], '--repair' in sys.argv
    src = os.path.join(os.environ['LOCALAPPDATA'], 'Nexdrop', 'nexdrop.db')
    copy = os.path.join(tempfile.gettempdir(), 'shift_copy.db')
    for e in ('', '-wal', '-shm'):
        if os.path.exists(src + e):
            shutil.copy2(src + e, copy + e)
    urls = {}
    for name, url, dest in sqlite3.connect(copy).execute("select filename,url,destination_path from downloads where filename like ?", (f'%{needle}%',)):
        urls[os.path.normcase(os.path.join(dest, name))] = url
    names = sorted(f for f in os.listdir(folder) if needle in f and f.endswith('.rar'))
    total_bad = 0
    for name in names:
        path = os.path.join(folder, name)
        runs, total = find_runs(path)
        if not runs:
            print(f'{name[-16:]}: clean', flush=True)
            continue
        total_bad += 1
        print(f'{name[-16:]}: {len(runs)} shifted stretch(es): ' + ', '.join(f'{s / MB:.2f}-{e / MB:.2f} MiB (shift {sh})' for s, e, sh in runs), flush=True)
        if repair:
            url = urls.get(os.path.normcase(path))
            if not url:
                print('   no link known for this file; skipped', flush=True)
                continue
            for s, e, _ in runs:
                lo = s - s % (64 * 1024)
                hi = min(total, e + 64 * 1024)
                failed = repair_range(path, url, lo, hi)
                if failed:
                    print('   could not re-fetch', failed, flush=True)
            again, _ = find_runs(path)
            print('   ' + ('repaired' if not again else f'STILL SHIFTED {again[:2]}'), flush=True)
    print('DONE', total_bad, 'file(s) had shifted stretches')


if __name__ == '__main__':
    main()
