"""Build the app icons from the Audiobookshelf logo (client/static/Logo.png, GPL-3.0).

Usage: python packaging/make_icon.py
"""

from pathlib import Path

from PIL import Image

OUT = Path(__file__).parent / "icons"
SOURCE = OUT / "audiobookshelf-logo.png"
ICO_SIZES = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]


def main():
    logo = Image.open(SOURCE).convert("RGBA")
    logo.resize((256, 256), Image.LANCZOS).save(OUT / "abs-desktop.png")
    logo.save(OUT / "abs-desktop.ico", sizes=ICO_SIZES)


if __name__ == "__main__":
    main()
