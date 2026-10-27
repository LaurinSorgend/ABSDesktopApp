"""Fill the AUR, Scoop and winget templates in packaging/ for one release.

Reads the release files from the dist folder to hash them and writes the manifests to
dist/manifests. Usage: python scripts/render-manifests.py v0.1.0 dist
"""

import hashlib
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TEMPLATES = {
    "aur/PKGBUILD.in": "aur/PKGBUILD",
    "scoop/abs-desktop.json.in": "scoop/abs-desktop.json",
    "winget/LaurinSorgend.ABSDesktop.yaml.in": "winget/LaurinSorgend.ABSDesktop.yaml",
    "winget/LaurinSorgend.ABSDesktop.installer.yaml.in": "winget/LaurinSorgend.ABSDesktop.installer.yaml",
    "winget/LaurinSorgend.ABSDesktop.locale.en-US.yaml.in": "winget/LaurinSorgend.ABSDesktop.locale.en-US.yaml",
}


def sha256(path):
    """Hex SHA-256 of a file."""
    digest = hashlib.sha256()
    with open(path, "rb") as file:
        for block in iter(lambda: file.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def main():
    version = sys.argv[1].removeprefix("v")
    dist = pathlib.Path(sys.argv[2])
    values = {
        "@VERSION@": version,
        "@SOURCE_SHA256@": sha256(dist / f"abs-desktop-{version}-source.tar.gz"),
        "@ZIP_SHA256@": sha256(dist / f"abs-desktop-{version}-windows-x64.zip"),
    }
    for template, target in TEMPLATES.items():
        text = (ROOT / "packaging" / template).read_text(encoding="utf-8")
        for key, value in values.items():
            text = text.replace(key, value)
        out = dist / "manifests" / target
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(text, encoding="utf-8", newline="\n")
        print(f"wrote {out}")


if __name__ == "__main__":
    main()
