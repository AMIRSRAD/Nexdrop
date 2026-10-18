"""Builds src-tauri/app-icon.png (1024x1024) from the Nexdrop logo artwork.

    python scripts/make_icon.py            # reads src-tauri/app-icon-source.webp
    pnpm tauri icon src-tauri/app-icon.png # regenerates src-tauri/icons/*

The source has a transparent margin around the tile; this trims it. Needs Pillow and numpy.
"""
import os
import numpy as np
from PIL import Image

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
im = Image.open(os.path.join(root, 'src-tauri', 'app-icon-source.webp')).convert('RGBA')
ys, xs = np.where(np.asarray(im.getchannel('A')) > 128)
tile = im.crop((xs.min(), ys.min(), xs.max() + 1, ys.max() + 1)).resize((1024, 1024), Image.LANCZOS)
tile.save(os.path.join(root, 'src-tauri', 'app-icon.png'))
print('wrote src-tauri/app-icon.png')
