"""Download and generate the public domain test media under fixtures/media.

Needs ffmpeg on PATH. Rerunning skips files that already exist.

Usage: python fixtures/fetch_fixtures.py
"""

import shutil
import subprocess
import zipfile
from pathlib import Path

MEDIA = Path(__file__).parent / "media"
AUDIO = MEDIA / "audiobooks"
EBOOKS = MEDIA / "ebooks"

LIBRIVOX_ID = "thehappyprinceandothertales_1411_librivox"
LIBRIVOX_FILES = [f"thehappyprinceandothertales_{n:02}_wilde_64kb.mp3" for n in range(1, 6)]
EPUB_URL = "https://www.gutenberg.org/ebooks/11.epub3.images"
PDF_URL = "https://archive.org/download/alicesadventur00carr/alicesadventur00carr_bw.pdf"

LONG_BOOK_HOURS = 40
LONG_BOOK_CHAPTER_MINUTES = 30
LONG_BOOK_ASIN = "B0FIXTURE1"
COMIC_PAGES = 24


def download(url, target):
    """Download url to target with curl unless it already exists."""
    if target.exists():
        return
    target.parent.mkdir(parents=True, exist_ok=True)
    print(f"downloading {url}")
    part = target.with_name(target.name + ".part")
    subprocess.run(["curl", "-fsSL", "--retry", "3", "-o", str(part), url], check=True)
    part.rename(target)


def ffmpeg(*args):
    """Run ffmpeg quietly and fail loudly."""
    subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *args], check=True)


def fetch_multi_file_book():
    """Multi-file MP3 book from LibriVox, no ASIN/ISBN."""
    folder = AUDIO / "Oscar Wilde" / "The Happy Prince and Other Tales"
    for name in LIBRIVOX_FILES:
        download(f"https://archive.org/download/{LIBRIVOX_ID}/{name}", folder / name)
    return folder


def write_chapter_metadata(path, title):
    """Write an ffmetadata file with fixed-length chapters for the long book."""
    chapter_ms = LONG_BOOK_CHAPTER_MINUTES * 60 * 1000
    count = LONG_BOOK_HOURS * 60 // LONG_BOOK_CHAPTER_MINUTES
    lines = [";FFMETADATA1", f"title={title}", "artist=Oscar Wilde", "album=" + title, f"asin={LONG_BOOK_ASIN}"]
    for i in range(count):
        lines += ["[CHAPTER]", "TIMEBASE=1/1000", f"START={i * chapter_ms}", f"END={(i + 1) * chapter_ms}", f"title=Chapter {i + 1}"]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def encode_source_pass(source_folder, work):
    """Encode the LibriVox files once into a single AAC file."""
    concat = work / "concat.txt"
    lines = [f"file '{(source_folder / n).resolve().as_posix()}'\n" for n in LIBRIVOX_FILES]
    concat.write_text("".join(lines), encoding="utf-8")
    base = work / "base.m4a"
    ffmpeg("-f", "concat", "-safe", "0", "-i", str(concat), "-ac", "1", "-c:a", "aac", "-b:a", "24k", str(base))
    return base


def build_long_m4b(source_folder):
    """40 h single-file M4B with chapters, made by looping the LibriVox audio."""
    title = "The Happy Prince Marathon"
    target = AUDIO / "Oscar Wilde" / title / f"{title}.m4b"
    if target.exists():
        return
    target.parent.mkdir(parents=True, exist_ok=True)
    part = target.with_name(target.name + ".part")
    work = MEDIA / ".work"
    work.mkdir(exist_ok=True)
    print(f"building {LONG_BOOK_HOURS} h M4B")
    base = encode_source_pass(source_folder, work)
    meta = work / "chapters.txt"
    write_chapter_metadata(meta, title)
    ffmpeg(
        "-stream_loop", "-1", "-i", str(base), "-i", str(meta),
        "-map", "0:a", "-map_metadata", "1", "-map_chapters", "1", "-t", str(LONG_BOOK_HOURS * 3600),
        "-c", "copy", "-movflags", "+faststart+use_metadata_tags", "-f", "mp4", str(part),
    )
    part.rename(target)
    shutil.rmtree(work)


def build_comic():
    """Generated CBZ with numbered test-pattern pages."""
    target = EBOOKS / "Fixture Studio" / "Test Pattern Comic" / "Test Pattern Comic.cbz"
    if target.exists():
        return
    pages = MEDIA / ".pages"
    pages.mkdir(parents=True, exist_ok=True)
    ffmpeg(
        "-f", "lavfi", "-i", f"testsrc2=size=1200x1800:rate=1:duration={COMIC_PAGES}",
        "-q:v", "4", str(pages / "page_%03d.jpg"),
    )
    target.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(target, "w", zipfile.ZIP_STORED) as cbz:
        for page in sorted(pages.glob("*.jpg")):
            cbz.write(page, page.name)
    shutil.rmtree(pages)


def main():
    source = fetch_multi_file_book()
    build_long_m4b(source)
    download(EPUB_URL, EBOOKS / "Lewis Carroll" / "Alice EPUB" / "Alice's Adventures in Wonderland.epub")
    download(PDF_URL, EBOOKS / "Lewis Carroll" / "Alice PDF" / "Alice's Adventures in Wonderland.pdf")
    build_comic()
    print("fixtures ready in", MEDIA)


if __name__ == "__main__":
    main()
