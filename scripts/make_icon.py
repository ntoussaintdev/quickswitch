"""Generate QuickSwitch icon assets (needs Pillow).

Writes:
  resources/quickswitch.ico      multi-size icon embedded in the exe and installer
  resources/icon_32.rgba         raw 32x32 RGBA for the tray icon
  resources/icon_64.rgba         raw 64x64 RGBA for the egui window icon

Run from the repository root:  python scripts/make_icon.py
"""

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "resources"
BIG = 1024  # draw large, then downsample for smooth edges


def draw_master() -> Image.Image:
    img = Image.new("RGBA", (BIG, BIG), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # Rounded square with a vertical indigo -> violet gradient.
    top, bottom = (79, 70, 229), (147, 51, 234)
    grad = Image.new("RGBA", (BIG, BIG))
    gd = ImageDraw.Draw(grad)
    for y in range(BIG):
        t = y / (BIG - 1)
        c = tuple(round(a + (b - a) * t) for a, b in zip(top, bottom))
        gd.line([(0, y), (BIG, y)], fill=c + (255,))
    mask = Image.new("L", (BIG, BIG), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [40, 40, BIG - 40, BIG - 40], radius=220, fill=255
    )
    img.paste(grad, (0, 0), mask)

    # Magnifier: ring + handle.
    white = (255, 255, 255, 255)
    cx, cy, r, w = 430, 430, 210, 78
    d.ellipse([cx - r, cy - r, cx + r, cy + r], outline=white, width=w)
    d.line([(cx + 150, cy + 150), (790, 790)], fill=white, width=110)
    d.ellipse([790 - 55, 790 - 55, 790 + 55, 790 + 55], fill=white)

    # Small "switch" chevron inside the lens.
    d.line([(370, 360), (460, 430), (370, 500)], fill=white, width=56, joint="curve")
    return img


def main() -> None:
    OUT.mkdir(exist_ok=True)
    master = draw_master()
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    master.save(OUT / "quickswitch.ico", sizes=[(s, s) for s in sizes])
    for s in (32, 64):
        small = master.resize((s, s), Image.LANCZOS)
        (OUT / f"icon_{s}.rgba").write_bytes(small.tobytes())
    print("icons written to", OUT)


if __name__ == "__main__":
    main()
