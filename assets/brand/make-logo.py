"""Turn the traced black-on-white logo into ink-on-transparent SVGs.

The trace stacks white shapes over black ones for the cut-out detail, so recolouring
shapes one by one would lose that detail. Instead the whole drawing becomes a luminance
mask (inverted, with a levels curve so ink is solid and paper is clear) over one solid
rect of the ink colour. The C2PA manifest is dropped: any edit invalidates its hash.

usage: python3 -I assets/brand/make-logo.py <original.svg> <out_dir>
"""
import re
import sys

src_path, out_dir = sys.argv[1], sys.argv[2]
src = open(src_path, encoding="utf-8").read()

# the drawing's square crop in the 2048 canvas (bbox 268..1776 x 302..1734, ~4% margin)
CROP = (208, 204, 1628, 1628)
INK_LO, PAPER_HI = 60, 200  # luminance <= LO is solid ink, >= HI is clear paper


def mask_gray(r: int, g: int, b: int) -> int:
    lum = 0.2126 * r + 0.7152 * g + 0.0722 * b
    paper = min(1.0, max(0.0, (lum - INK_LO) / (PAPER_HI - INK_LO)))
    return round(255 * (1 - paper))


def recolour(m: re.Match) -> str:
    attr, r, g, b = m.group(1), int(m.group(2)), int(m.group(3)), int(m.group(4))
    v = mask_gray(r, g, b)
    return f'{attr}="rgb({v},{v},{v})"'


open_end = src.index(">", src.index("<svg")) + 1
body = src[open_end : src.rindex("</svg>")]
body = re.sub(r"<metadata>.*?</metadata>", "", body, flags=re.S)
body = re.sub(r'(fill|stop-color)="rgb\((\d+),\s*(\d+),\s*(\d+)\)"', recolour, body)
assert "rgb(" in body and "<path" in body
leftover = re.findall(r'(?:fill|stop-color)="(?!rgb\(|url\()[^"]*"', body)
assert not leftover, leftover

# the trace's own <defs> (gradients) stay outside the mask; its paths go inside
defs_match = re.search(r"<defs>.*?</defs>", body, flags=re.S)
defs = defs_match.group(0) if defs_match else ""
paths = body.replace(defs, "") if defs else body
inner_defs = defs[len("<defs>") : -len("</defs>")] if defs else ""


def masked(ink: str, mask_id: str) -> str:
    x, y, w, h = CROP
    return (
        f"<defs>{inner_defs}"
        f'<mask id="{mask_id}" maskUnits="userSpaceOnUse" x="0" y="0" width="2048" height="2048"'
        f' style="mask-type:luminance">'
        f'<rect width="2048" height="2048" fill="#000"/>{paths}</mask></defs>'
        f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{ink}" mask="url(#{mask_id})"/>'
    )


def logo(ink: str) -> str:
    x, y, w, h = CROP
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" '
        f'viewBox="{x} {y} {w} {h}" width="1024" height="1024">'
        "<title>Zeiler</title>" + masked(ink, "zeiler-ink") + "</svg>\n"
    )


def app_icon(tile: str, ink: str) -> str:
    # the white ship on a dark tile, for the OS icons (a transparent white icon
    # would vanish on light taskbars and in Finder); the ship fills ~82% of the tile
    x, y, w, h = CROP
    size, scale = 1024, 0.82
    inner = round(size * scale)
    off = (size - inner) // 2
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" '
        f'width="{size}" height="{size}">'
        f'<rect width="{size}" height="{size}" fill="{tile}"/>'
        f'<svg x="{off}" y="{off}" width="{inner}" height="{inner}" viewBox="{x} {y} {w} {h}">'
        + masked(ink, "zeiler-ink")
        + "</svg></svg>\n"
    )


for name, text in [
    ("zeiler-logo-white.svg", logo("#FFFFFF")),
    ("zeiler-logo-black.svg", logo("#000000")),
    ("app-icon.svg", app_icon("#111213", "#FFFFFF")),
]:
    with open(f"{out_dir}/{name}", "w", encoding="utf-8") as f:
        f.write(text)
    print(name, len(text))
