# Zeiler logo

The woodcut pirate ship, generated with an image model (Higgsfield / Recraft) and traced
to SVG, as black ink on white paper (2048 × 2048).

| File | What it is | Used by |
|---|---|---|
| `zeiler-logo-white.svg` | White ink on a transparent background, cropped square to the drawing | Master for dark backgrounds |
| `zeiler-logo-black.svg` | Black ink on a transparent background, same crop | Master for light backgrounds |
| `zeiler-logo-white.png`, `zeiler-logo-black.png` | 512 px renders of the two | README header (dark and light theme) |
| `make-logo.py` | Makes the two SVGs (and the app-icon SVG) from the traced original | |

Elsewhere: `src/assets/zeiler-logo-white.png` (the sidebar), `website/public/logo-*.png`
(the site header), and `app-icon.png` at the repo root, which is the white ship on
Zeiler's canvas colour (`#111213`). `app-icon.png` is the source of every OS icon in
`src-tauri/icons/` and of the site's `favicon.png` and `apple-touch-icon.png`. The OS icon
has a dark tile on purpose: a white ship on a transparent icon would vanish on light
taskbars, in Finder and in the installer.

## How the SVGs are made

The trace stacks white shapes over black ones for the cut-out detail (the hatching, the
skull), so the colours cannot be swapped shape by shape. `make-logo.py` instead puts the
whole drawing in an SVG luminance mask, inverted with a levels curve (ink solid, paper
clear, greys partly clear), over a single rect of the ink colour. It also drops the
trace's C2PA manifest, which no longer matches once the file is edited.

```bash
python3 -I assets/brand/make-logo.py <traced-original.svg> <out-dir>
```

## Regenerating the icons and PNGs

Tauri's icon tool renders SVG (masks included) with transparency:

```bash
npx tauri icon <out-dir>/app-icon.svg -o <tmp> -p 1024 -p 180 -p 64  # app-icon.png, apple-touch, favicon
npx tauri icon app-icon.png                                           # every OS icon in src-tauri/icons
npx tauri icon assets/brand/zeiler-logo-white.svg -o <tmp> -p 512 -p 256
```
