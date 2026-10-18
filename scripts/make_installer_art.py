"""Renders the NSIS wizard artwork (header + sidebar) from the app icon."""
import os
from PIL import Image, ImageDraw

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
icon = Image.open(os.path.join(root, 'src-tauri', 'app-icon.png')).convert('RGBA')
out = os.path.join(root, 'src-tauri', 'installer')
os.makedirs(out, exist_ok=True)

# Header: 150x57, white, icon on the right (NSIS shows the title text on the left).
header = Image.new('RGB', (150, 57), 'white')
small = icon.resize((44, 44), Image.LANCZOS)
header.paste(small, (100, 6), small)
header.save(os.path.join(out, 'header.bmp'))

# Sidebar: 164x314, the app's dark background with the icon near the top.
side = Image.new('RGB', (164, 314), (13, 14, 17))
d = ImageDraw.Draw(side)
big = icon.resize((96, 96), Image.LANCZOS)
side.paste(big, (34, 40), big)
d.rectangle((0, 308, 164, 314), fill=(59, 130, 246))
side.save(os.path.join(out, 'sidebar.bmp'))
