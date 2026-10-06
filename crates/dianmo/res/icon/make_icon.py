#!/usr/bin/env python3
"""Build crates/dianmo/res/dianmo.ico (and optional previews) from the SVG sources here.

    uv run --with cairosvg --with pillow python3 crates/dianmo/res/icon/make_icon.py [--previews]

Sizes <= SMALL_MAX are rendered from dianmo-small.svg (full-bleed tile, no sheen/shadow, bigger
glyph) so they stay crisp in the tray and small shell views; larger sizes come from dianmo.svg.
Every image is rendered at its exact pixel size (no downscaling of a big bitmap). The ICO stores
32-bit BMP (DIB) entries for 16..128 and a PNG entry for 256, which every Windows since Vista
accepts. `--previews` also writes docs/previews/icon-*.png (chosen icon, size/taskbar check and
the alternative concepts in concepts/).
"""

import io
import struct
import sys
from pathlib import Path

import cairosvg
from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent
RES = HERE.parent
REPO = RES.parents[2]
PREVIEWS = REPO / "docs" / "previews"

SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
SMALL_MAX = 32

CONCEPTS = [
    # (key, title, master svg, small svg)
    ("brush", "A 笔锋一点（选用）", HERE / "dianmo.svg", HERE / "dianmo-small.svg"),
    ("ripple", "B 墨滴·涟漪", HERE / "concepts" / "ripple.svg", HERE / "concepts" / "ripple-small.svg"),
    ("keycap", "C 键帽·墨滴", HERE / "concepts" / "keycap.svg", HERE / "concepts" / "keycap-small.svg"),
]


def render(svg: Path, size: int) -> Image.Image:
    png = cairosvg.svg2png(url=str(svg), output_width=size, output_height=size)
    return Image.open(io.BytesIO(png)).convert("RGBA")


def render_icon(master: Path, small: Path, size: int) -> Image.Image:
    return render(small if size <= SMALL_MAX else master, size)


# ---------------------------------------------------------------- ICO writer


def dib(im: Image.Image) -> bytes:
    """32-bit BGRA DIB with AND mask, as stored inside .ico files (height doubled, bottom-up)."""
    w, h = im.size
    header = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    px = im.tobytes("raw", "BGRA")
    stride = w * 4
    xor = b"".join(px[y * stride:(y + 1) * stride] for y in range(h - 1, -1, -1))
    alpha = im.getchannel("A").tobytes()
    mask_stride = ((w + 31) // 32) * 4
    rows = []
    for y in range(h - 1, -1, -1):
        row = bytearray(mask_stride)
        for x in range(w):
            if alpha[y * w + x] == 0:
                row[x // 8] |= 0x80 >> (x % 8)
        rows.append(bytes(row))
    return header + xor + b"".join(rows)


def png_bytes(im: Image.Image) -> bytes:
    buf = io.BytesIO()
    im.save(buf, "PNG", optimize=True)
    return buf.getvalue()


def write_ico(path: Path, images: list[Image.Image]) -> None:
    blobs = [png_bytes(im) if im.width >= 256 else dib(im) for im in images]
    out = bytearray(struct.pack("<HHH", 0, 1, len(images)))
    offset = 6 + 16 * len(images)
    for im, blob in zip(images, blobs):
        dim = 0 if im.width >= 256 else im.width
        out += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    for blob in blobs:
        out += blob
    path.write_bytes(bytes(out))


# ---------------------------------------------------------------- previews

FONT_PATHS = [
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "C:/Windows/Fonts/msyh.ttc",
]


def font(px: int):
    for p in FONT_PATHS:
        try:
            return ImageFont.truetype(p, px)
        except OSError:
            pass
    return ImageFont.load_default()


LIGHT_BG = (243, 243, 243, 255)   # Windows light taskbar
DARK_BG = (28, 28, 28, 255)       # Windows dark taskbar
PAGE_BG = (250, 250, 252, 255)
INK = (40, 40, 48, 255)
MUTED = (120, 120, 132, 255)


def size_strip(master: Path, small: Path) -> Image.Image:
    """All ICO sizes at 1:1 on light and dark taskbar bands, plus 16/20/24/32 magnified 4x."""
    pad = 24
    sizes = SIZES[:-1]
    mag = [16, 20, 24, 32]
    w = max(sum(s + 20 for s in sizes), sum((s + 2) * 8 + 24 for s in mag)) + pad * 2
    band_h = 24 + 128 + 24
    h = 16 + band_h * 2 + 16 + 28 + (mag[-1] + 2) * 4 + 24
    sheet = Image.new("RGBA", (w, h), PAGE_BG)
    d = ImageDraw.Draw(sheet)
    f = font(16)
    y = 16
    for label, bg, fg in (("浅色任务栏", LIGHT_BG, INK), ("深色任务栏", DARK_BG, (230, 230, 230, 255))):
        sheet.paste(bg, (0, y, w, y + band_h))
        d.text((pad, y + 2), label, font=f, fill=fg)
        x = pad
        for s in sizes:
            sheet.alpha_composite(render_icon(master, small, s), (x, y + 24 + 128 - s))
            d.text((x, y + 24 + 128 + 3), str(s), font=font(12), fill=fg)
            x += s + 20
        y += band_h
    y += 16
    d.text((pad, y), "16 / 20 / 24 / 32 放大 4 倍（逐像素，浅色 / 深色）", font=f, fill=INK)
    y += 28
    x = pad
    for s in mag:
        for bg in (LIGHT_BG, DARK_BG):
            tile = Image.new("RGBA", (s + 2, s + 2), bg)
            tile.alpha_composite(render_icon(master, small, s), (1, 1))
            sheet.alpha_composite(tile.resize(((s + 2) * 4, (s + 2) * 4), Image.NEAREST), (x, y))
            x += (s + 2) * 4 + 4
        x += 16
    return sheet


def tray_mock(master: Path, small: Path) -> Image.Image:
    """Rough Windows 10 tray area (100% and 200% scaling) on light and dark taskbars."""
    w, h = 560, 4 * 64 + 40
    sheet = Image.new("RGBA", (w, h), PAGE_BG)
    d = ImageDraw.Draw(sheet)
    y = 20
    for scale, s, bar in ((1, 16, 40), (2, 32, 80)):
        for bg, fg in ((LIGHT_BG, INK), (DARK_BG, (235, 235, 235, 255))):
            bh = bar if scale == 2 else 40
            sheet.paste(bg, (0, y, w, y + bh))
            d.text((16, y + bh // 2 - 9), f"{scale * 100}%", font=font(14 * scale // 1 if scale == 1 else 20), fill=fg)
            x = 140
            # neighbour glyphs: up-arrow, a grey generic icon, then ours, then clock text
            d.polygon([(x, y + bh // 2 + 3 * scale), (x + 5 * scale, y + bh // 2 - 3 * scale),
                       (x + 10 * scale, y + bh // 2 + 3 * scale)], outline=fg)
            x += 30 * scale
            d.rounded_rectangle((x, y + (bh - s) // 2, x + s - 1, y + (bh - s) // 2 + s - 1),
                                radius=2 * scale, outline=MUTED, width=scale)
            x += s + 14 * scale
            sheet.alpha_composite(render_icon(master, small, s), (x, y + (bh - s) // 2))
            x += s + 14 * scale
            d.text((x, y + bh // 2 - 8 * scale), "中", font=font(14 * scale), fill=fg)
            x += 30 * scale
            d.text((x, y + bh // 2 - 8 * scale), "14:32", font=font(13 * scale), fill=fg)
            y += bh + 6
        y += 10
    return sheet.crop((0, 0, w, y + 10))


def concept_card(title: str, master: Path, small: Path) -> Image.Image:
    w, h = 400, 620
    card = Image.new("RGBA", (w, h), PAGE_BG)
    d = ImageDraw.Draw(card)
    d.text((24, 16), title, font=font(22), fill=INK)
    card.alpha_composite(render(master, 256), ((w - 256) // 2, 60))
    y = 340
    for bg in (LIGHT_BG, DARK_BG):
        card.paste(bg, (0, y, w, y + 72))
        x = 24
        for s in (48, 32, 24, 20, 16):
            card.alpha_composite(render_icon(master, small, s), (x, y + 12 + (48 - s)))
            x += s + 18
        y += 72
    mx = 24
    for s in (16, 24, 32):
        big = render_icon(master, small, s).resize((s * 3, s * 3), Image.NEAREST)
        card.alpha_composite(big, (mx, y + 16))
        mx += s * 3 + 16
    return card


def write_previews() -> None:
    PREVIEWS.mkdir(parents=True, exist_ok=True)
    key, _, master, small = CONCEPTS[0]
    # Hero: large icon on light and dark backgrounds.
    hero = Image.new("RGBA", (1100, 560), PAGE_BG)
    hero.paste(DARK_BG, (550, 0, 1100, 560))
    big = render(master, 512)
    hero.alpha_composite(big, (19, 24))
    hero.alpha_composite(big, (569, 24))
    hero.save(PREVIEWS / "icon-main.png", optimize=True)
    size_strip(master, small).save(PREVIEWS / "icon-sizes.png", optimize=True)
    tray_mock(master, small).save(PREVIEWS / "icon-tray.png", optimize=True)
    cards = [concept_card(t, m, s) for _, t, m, s in CONCEPTS]
    sheet = Image.new("RGBA", (400 * len(cards), 620), PAGE_BG)
    for i, c in enumerate(cards):
        sheet.alpha_composite(c, (400 * i, 0))
    sheet.save(PREVIEWS / "icon-concepts.png", optimize=True)
    for k, t, m, s in CONCEPTS[1:]:
        concept_card(t, m, s).save(PREVIEWS / f"icon-alt-{k}.png", optimize=True)


def main() -> None:
    _, _, master, small = CONCEPTS[0]
    images = [render_icon(master, small, s) for s in SIZES]
    write_ico(RES / "dianmo.ico", images)
    print(f"wrote {RES / 'dianmo.ico'} ({(RES / 'dianmo.ico').stat().st_size} bytes, sizes {SIZES})")
    if "--previews" in sys.argv:
        write_previews()
        print(f"wrote previews to {PREVIEWS}/icon-*.png")


if __name__ == "__main__":
    main()
