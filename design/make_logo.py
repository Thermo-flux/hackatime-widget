"""Hackatime Widget logo: "Tock", a little stopwatch with a mascot face.

Pixel-art source of truth for every icon size. Run from the repo root:

    python design/make_logo.py master   # writes app-icon.png (1024x1024, padded)
    cargo tauri icon app-icon.png       # generates the full set (then delete src-tauri/icons/android, ios)
    python design/make_logo.py icons    # overwrites the set with crisp nearest-neighbour renders
    python design/make_logo.py design   # writes design/logo.svg, design/banner.png, design/logo-preview.png

Two hand-tuned grids: a 16x16 one (used for 16-48 px, so the .ico stays crisp at taskbar sizes) and a
32x32 one (64 px and up). Every size is an integer nearest-neighbour scale of one of them: no blur.
Needs Pillow.
"""
import math
import os
import struct
import sys
from io import BytesIO

from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICONS = os.path.join(ROOT, "src-tauri", "icons")

PAL = {
    "k": "#1b1b2a",  # ink outline
    "r": "#ec3750",  # Hack Club red: elapsed part of the timer ring
    "R": "#b0223e",  # ring shade
    "q": "#ffc2cd",  # pale pink: remaining part of the ring
    "w": "#ffffff",  # clock face
    "W": "#dde3f2",  # face shade
    "p": "#ff9db8",  # blush (same as the classic mascot)
}

S16 = [
    "......kkkk......",
    "......krrk......",
    ".....kkkkkk.....",
    "...kkqqrrrrkk...",
    "..kqqqrrrrrrrk..",
    ".kqqqwwwwwwrrrk.",
    ".kqqwwwwwwwwrrk.",
    ".kqwwkkwwkkwwrk.",
    ".kqwwkkwwkkwwrk.",
    ".krwwwwwwwwwwrk.",
    ".krwpwkwwkwpwrk.",
    ".krrwwwkkwwwrRk.",
    ".krrrwwwwwwrRRk.",
    "..krrrrrrrrRRk..",
    "...kkrrrrRRkk...",
    ".....kkkkkk.....",
]


def _build32():
    n = 32
    g = [["."] * n for _ in range(n)]
    cx, cy = 16.0, 18.0

    def disk(r):
        return lambda x, y: math.hypot(x + 0.5 - cx, y + 0.5 - cy) <= r

    outer, inner = disk(13.0), disk(9.0)

    def edge(f, x, y):  # inner boundary with 4-neighbourhood = clean 1px 8-connected outline
        return f(x, y) and any(not f(x + dx, y + dy) for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)))

    for y in range(n):
        for x in range(n):
            if not outer(x, y):
                continue
            px, py = x + 0.5 - cx, y + 0.5 - cy
            a = (math.degrees(math.atan2(px, -py)) + 360) % 360  # clockwise from 12 o'clock
            if edge(outer, x, y) or edge(inner, x, y):
                g[y][x] = "k"
            elif inner(x, y):
                g[y][x] = "W" if px + py > 8.6 else "w"
            else:
                g[y][x] = "q" if a >= 270 else ("R" if px + py > 8 else "r")
    g[15][27], g[29][12] = "r", "R"  # tidy the ends of the shade line
    for y in range(4):  # crown button
        for x in range(13, 19):
            g[y][x] = "k" if (y in (0, 3) or x in (13, 18)) else "r"
    g[4][15] = g[4][16] = "k"  # stem
    for ex in (11, 19):  # eyes 2x3
        for dy in range(3):
            g[15 + dy][ex] = g[15 + dy][ex + 1] = "k"
    for x in (9, 10, 21, 22):  # cheeks
        g[19][x] = "p"
    for x, y in ((14, 19), (17, 19), (15, 20), (16, 20)):  # smile
        g[y][x] = "k"
    return ["".join(r) for r in g]


S32 = _build32()


def rgba(hex_):
    return tuple(int(hex_[i:i + 2], 16) for i in (1, 3, 5)) + (255,)


def render(grid, scale):
    n = len(grid)
    small = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    for y, row in enumerate(grid):
        assert len(row) == n, (y, row)
        for x, ch in enumerate(row):
            if ch != ".":
                small.putpixel((x, y), rgba(PAL[ch]))
    return small.resize((n * scale, n * scale), Image.NEAREST)


def on_canvas(art, size):
    c = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    c.alpha_composite(art, ((size - art.width) // 2, (size - art.height) // 2))
    return c


def full(size):
    """Largest crisp render that fits `size` (Windows/Linux icons: full bleed)."""
    if size < 64:
        return on_canvas(render(S16, max(1, min(3, size // 16))), size)
    return on_canvas(render(S32, size // 32), size)


def padded(size):
    """macOS-style render with ~6-12% padding (icns, master)."""
    if size <= 32:
        return full(size)
    if size == 64:
        return on_canvas(render(S16, 3), 64)
    return on_canvas(render(S32, (size * 7 // 8) // 32), size)


def write_ico(path, sizes=(16, 24, 32, 48, 64, 128, 256)):
    """ICO with classic 32-bit BMP entries (<256) and a PNG entry for 256: maximally compatible."""
    blobs = []
    for s in sizes:
        im = full(s)
        if s >= 256:
            b = BytesIO()
            im.save(b, "PNG")
            blobs.append(b.getvalue())
            continue
        hdr = struct.pack("<IiiHHIIiiII", 40, s, s * 2, 1, 32, 0, 0, 0, 0, 0, 0)
        px = im.load()
        xor = bytearray()
        for y in range(s - 1, -1, -1):
            for x in range(s):
                r, g, b_, a = px[x, y]
                xor += bytes((b_, g, r, a))
        row = ((s + 31) // 32) * 4
        andm = bytearray()
        for y in range(s - 1, -1, -1):
            bits = bytearray(row)
            for x in range(s):
                if px[x, y][3] == 0:
                    bits[x // 8] |= 0x80 >> (x % 8)
            andm += bits
        blobs.append(hdr + bytes(xor) + bytes(andm))
    out = struct.pack("<HHH", 0, 1, len(sizes))
    off = 6 + 16 * len(sizes)
    for s, blob in zip(sizes, blobs):
        d = 0 if s >= 256 else s
        out += struct.pack("<BBBBHHII", d, d, 0, 0, 1, 32, len(blob), off)
        off += len(blob)
    with open(path, "wb") as f:
        f.write(out + b"".join(blobs))


def master():
    padded(1024).save(os.path.join(ROOT, "app-icon.png"))


def icons():
    for name in os.listdir(ICONS):  # keep each PNG `cargo tauri icon` made, at its size, but re-render it crisply
        p = os.path.join(ICONS, name)
        if name.endswith(".png"):
            with Image.open(p) as im:
                size = im.width
            full(size).save(p)
    write_ico(os.path.join(ICONS, "icon.ico"))
    sizes = [16, 32, 64, 128, 256, 512, 1024]
    imgs = [padded(s) for s in sizes]
    icns = os.path.join(ICONS, "icon.icns")
    imgs[-1].save(icns, append_images=imgs[:-1])
    # Pillow skips the 1x 16/32 slots; add them (icp4/icp5, PNG) so small Finder views use the 16-grid art.
    with open(icns, "rb") as f:
        data = f.read()
    chunks, p = [], 8
    while p < len(data):
        n = struct.unpack(">I", data[p + 4:p + 8])[0]
        if data[p:p + 4] != b"TOC ":
            chunks.append(data[p:p + n])
        p += n
    for tag, s in ((b"icp4", 16), (b"icp5", 32)):
        b = BytesIO()
        full(s).save(b, "PNG")
        chunks.append(tag + struct.pack(">I", 8 + len(b.getvalue())) + b.getvalue())
    toc = b"".join(c[:8] for c in chunks)  # TOC lists every entry's type + length
    body = b"TOC " + struct.pack(">I", 8 + len(toc)) + toc + b"".join(chunks)
    with open(icns, "wb") as f:
        f.write(b"icns" + struct.pack(">I", 8 + len(body)) + body)


def svg():
    rects = []
    for y, row in enumerate(S32):
        x = 0
        while x < len(row):
            ch = row[x]
            if ch == ".":
                x += 1
                continue
            x0 = x
            while x < len(row) and row[x] == ch:
                x += 1
            rects.append(f'<rect x="{x0}" y="{y}" width="{x - x0}" height="1" fill="{PAL[ch]}"/>')
    body = "\n  ".join(rects)
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="256" height="256" '
        'shape-rendering="crispEdges">\n  <title>Hackatime Widget</title>\n  ' + body + "\n</svg>\n"
    )


# 5x8 pixel font (baseline on row 6, descenders on row 7).
FONT = {
    "H": "#...#|#...#|#...#|#####|#...#|#...#|#...#|.....",
    "W": "#...#|#...#|#...#|#.#.#|#.#.#|#.#.#|.#.#.|.....",
    "a": ".....|.....|.###.|....#|.####|#...#|.####|.....",
    "c": ".....|.....|.####|#....|#....|#....|.####|.....",
    "d": "....#|....#|.####|#...#|#...#|#...#|.####|.....",
    "e": ".....|.....|.###.|#...#|#####|#....|.###.|.....",
    "g": ".....|.....|.####|#...#|#...#|.####|....#|.###.",
    "h": "#....|#....|####.|#...#|#...#|#...#|#...#|.....",
    "i": "..#..|.....|.##..|..#..|..#..|..#..|.###.|.....",
    "k": "#....|#....|#..#.|#.#..|##...|#.#..|#..#.|.....",
    "l": ".##..|..#..|..#..|..#..|..#..|..#..|.###.|.....",
    "m": ".....|.....|##.#.|#.#.#|#.#.#|#.#.#|#.#.#|.....",
    "n": ".....|.....|####.|#...#|#...#|#...#|#...#|.....",
    "o": ".....|.....|.###.|#...#|#...#|#...#|.###.|.....",
    "p": ".....|.....|####.|#...#|#...#|####.|#....|#....",
    "r": ".....|.....|#.##.|##..#|#....|#....|#....|.....",
    "s": ".....|.....|.####|#....|.###.|....#|####.|.....",
    "t": ".#...|.#...|####.|.#...|.#...|.#...|..##.|.....",
    "u": ".....|.....|#...#|#...#|#...#|#...#|.####|.....",
    "y": ".....|.....|#...#|#...#|#...#|.####|....#|.###.",
    ",": ".....|.....|.....|.....|.....|..#..|..#..|.#...",
    " ": ".....|.....|.....|.....|.....|.....|.....|.....",
}


def text(img, s, x, y, scale, color, shadow=None):
    for ch in s:
        rows = FONT[ch].split("|")
        for dy, row in enumerate(rows):
            for dx, on in enumerate(row):
                if on == "#":
                    for col, off in ((shadow, scale), (color, 0)):
                        if col is None:
                            continue
                        px, py = x + dx * scale + off, y + dy * scale + off
                        img.paste(rgba(col), (px, py, px + scale, py + scale))
        x += 6 * scale
    return x


def text_width(s, scale):
    return len(s) * 6 * scale - scale


def sticker(art, color):
    """Add a 1px outline around the art so the dark ink edge still reads on a dark background."""
    out = art.copy()
    src, dst = art.load(), out.load()
    for y in range(art.height):
        for x in range(art.width):
            if src[x, y][3] == 0 and any(
                0 <= x + dx < art.width and 0 <= y + dy < art.height and src[x + dx, y + dy][3]
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1))
            ):
                dst[x, y] = rgba(color)
    return out


def banner():
    w, h = 1200, 300
    img = Image.new("RGBA", (w, h), rgba("#17171d"))
    dot = rgba("#23232c")
    for y in range(6, h, 12):  # faint pixel grid
        for x in range(6, w, 12):
            img.paste(dot, (x, y, x + 2, y + 2))
    logo = sticker(render(S32, 1), "#3a3a50").resize((32 * 7, 32 * 7), Image.NEAREST)  # 224 px
    title_s, tag_s = 8, 3
    title_w = text_width("Hackatime Widget", title_s)
    total = logo.width + 48 + title_w
    x0 = (w - total) // 2
    img.alpha_composite(logo, (x0, (h - logo.height) // 2))
    tx = x0 + logo.width + 48
    ty = 88
    x = text(img, "Hackatime ", tx, ty, title_s, "#ffffff", shadow="#34344a")
    text(img, "Widget", x, ty, title_s, "#ec3750", shadow="#7a1528")
    text(img, "your coding time, right on your desktop", tx + 3, ty + 8 * title_s + 30, tag_s, "#9aa3bd")
    strip = [rgba(c) for c in ("#ec3750", "#ff8c37", "#f1c40f", "#33d6a6", "#5bc0de", "#338eda", "#a633d6")]
    seg = w // len(strip) + 1
    for i, c in enumerate(strip):
        img.paste(c, (i * seg, h - 8, min(w, (i + 1) * seg), h))
    return img


def design():
    d = os.path.join(ROOT, "design")
    with open(os.path.join(d, "logo.svg"), "w", encoding="utf-8", newline="\n") as f:
        f.write(svg())
    banner().convert("RGB").save(os.path.join(d, "banner.png"))
    # Preview sheet: every icon size on light and dark backgrounds.
    sizes = [16, 24, 32, 48, 64, 128, 256]
    sheet = Image.new("RGBA", (sum(sizes) + 20 * (len(sizes) + 1), 2 * 276), rgba("#f4f4f8"))
    sheet.paste(rgba("#202024"), (0, 276, sheet.width, 552))
    for row in range(2):
        x = 20
        for s in sizes:
            sheet.alpha_composite(full(s), (x, row * 276 + 10))
            x += s + 20
    sheet.save(os.path.join(d, "logo-preview.png"))


if __name__ == "__main__":
    {"master": master, "icons": icons, "design": design}[sys.argv[1]]()
