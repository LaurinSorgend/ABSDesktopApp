"""Draw the app icon: a blue square with a white play triangle, like the tray icon.

Usage: python packaging/make_icon.py
"""

from pathlib import Path

from PIL import Image, ImageDraw

BLUE = (30, 102, 245)
OUT = Path(__file__).parent / "icons"


def draw(size):
    """The icon at `size` pixels, drawn at 4x and scaled down for smooth edges."""
    big = size * 4
    image = Image.new("RGBA", (big, big), BLUE + (255,))
    s = big / 32
    triangle = [(11 * s, 8 * s), (11 * s, 24 * s), (24 * s, 16 * s)]
    ImageDraw.Draw(image).polygon(triangle, fill=(255, 255, 255, 255))
    return image.resize((size, size), Image.LANCZOS)


def main():
    OUT.mkdir(exist_ok=True)
    draw(256).save(OUT / "abs-desktop.png")
    draw(256).save(OUT / "abs-desktop.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (256, 256)])


if __name__ == "__main__":
    main()
