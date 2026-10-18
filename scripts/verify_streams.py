"""Decodes the files that tests/stream_real.rs downloaded, to prove they play.

    pip install av
    python scripts/verify_streams.py [folder]
"""
import os, sys
import av

folder = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'src-tauri', 'target', 'real_streams')
ok = True
for name in sorted(os.listdir(folder)):
    path = os.path.join(folder, name)
    try:
        with av.open(path) as c:
            streams = [(s.type, s.codec_context.name, getattr(s, 'width', None), getattr(s, 'height', None)) for s in c.streams]
            frames = {'video': 0, 'audio': 0}
            last_pts = {}
            for packet in c.demux():
                if packet.size == 0:
                    continue
                for frame in packet.decode():
                    kind = 'video' if isinstance(frame, av.VideoFrame) else 'audio'
                    frames[kind] += 1
            dur = float(c.duration / av.time_base) if c.duration else 0
        print(f'OK   {name}: {streams} duration={dur:.1f}s decoded video frames={frames["video"]} audio frames={frames["audio"]}')
    except Exception as e:  # noqa: BLE001
        ok = False
        print(f'FAIL {name}: {type(e).__name__}: {e}')
sys.exit(0 if ok else 1)
