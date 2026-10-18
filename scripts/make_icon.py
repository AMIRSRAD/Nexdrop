"""Renders the Nexdrop app icon (1024x1024 PNG) used to generate every platform icon.

    python scripts/make_icon.py            # writes src-tauri/app-icon.png
    pnpm tauri icon src-tauri/app-icon.png # regenerates src-tauri/icons/*

The design: a rounded blue tile with a white "download" arrow over a base line.
"""
import os
from PIL import Image, ImageDraw

SIZE = 1024
SS = 4  # supersampling for smooth edges
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def main():
    s = SIZE * SS
    # Diagonal gradient tile
    top, bottom = (122, 162, 255), (46, 94, 245)
    grad = Image.new('RGB', (s, s))
    px = grad.load()
    # Build per-row/col gradient cheaply: diagonal t = (x+y)/(2s)
    row = [lerp(top, bottom, i / (2 * s)) for i in range(2 * s)]
    for y in range(s):
        for x in range(s):
            px[x, y] = row[x + y]
    mask = Image.new('L', (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, s - 1, s - 1], radius=int(s * 0.225), fill=255)
    tile = Image.new('RGBA', (s, s), (0, 0, 0, 0))
    tile.paste(grad, (0, 0), mask)

    d = ImageDraw.Draw(tile)
    u = s / 24.0
    w = int(2.7 * u)  # stroke width
    white = (255, 255, 255, 255)

    def line(x1, y1, x2, y2, fill=white):
        d.line([(x1 * u, y1 * u), (x2 * u, y2 * u)], fill=fill, width=w)
        r = w / 2
        for x, y in ((x1, y1), (x2, y2)):
            d.ellipse([x * u - r, y * u - r, x * u + r, y * u + r], fill=fill)

    # Arrow (shaft + head) and base line, on the 24-unit grid
    line(12, 4.6, 12, 14.4)
    line(12, 14.4, 7.6, 10.0)
    line(12, 14.4, 16.4, 10.0)
    line(6.4, 18.6, 17.6, 18.6, fill=(255, 255, 255, 235))

    out = tile.resize((SIZE, SIZE), Image.LANCZOS)
    dest = os.path.join(ROOT, 'src-tauri', 'app-icon.png')
    out.save(dest)
    print('wrote', dest)


if __name__ == '__main__':
    main()
