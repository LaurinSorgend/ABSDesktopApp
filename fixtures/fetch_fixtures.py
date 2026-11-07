"""Generate the test media under fixtures/media.

By default every file is generated, so CI needs no download. With --real the
multi-file book and the Alice ebooks are the public domain originals from
LibriVox, Project Gutenberg and archive.org instead.

Needs ffmpeg on PATH, and curl for --real. Rerunning skips files that already exist.

Usage: python fixtures/fetch_fixtures.py [--real]
"""

import argparse
import shutil
import subprocess
import zipfile
from pathlib import Path

import fake_ebooks

MEDIA = Path(__file__).parent / "media"
AUDIO = MEDIA / "audiobooks"
EBOOKS = MEDIA / "ebooks"
GRID = MEDIA / "grid"
MATCHING = MEDIA / "matching"

LIBRIVOX_ID = "thehappyprinceandothertales_1411_librivox"
LIBRIVOX_FILES = [f"thehappyprinceandothertales_{n:02}_wilde_64kb.mp3" for n in range(1, 6)]
LIBRIVOX_COVER = "happy_prince_and_other_tales_1411.jpg"
MULTI_FILE_ALBUM = "The Happy Prince and Other Tales"
# Track title and length in seconds of the LibriVox files, copied by the generated ones.
MULTI_FILE_TRACKS = [
    ("The Happy Prince", 1440.78),
    ("The Nightingale and the Rose", 956.03),
    ("The Selfish Giant", 680.78),
    ("The Devoted Friend", 1651.98),
    ("The Remarkable Rocket", 1952.71),
]
EPUB_URL = "https://www.gutenberg.org/ebooks/11.epub3.images"
PDF_URL = "https://archive.org/download/alicesadventur00carr/alicesadventur00carr_bw.pdf"

LONG_BOOK_HOURS = 40
LONG_BOOK_CHAPTER_MINUTES = 30
LONG_BOOK_ASIN = "B0FIXTURE1"
COMIC_PAGES = 24
GRID_ITEMS = 500

# Books for the work matching tests: folder (author, title) and length in minutes.
# docker/init_server.py gives some of them different metadata on each server.
MATCHING_BOOKS = [
    ("Fixture Author", "Shared Narration", 10),
    ("Fixture Author", "Two Narrators", 10),
    ("Fixture Author", "Fixture Tale", 12),
    ("Fixture Author", "Fixture Tale Abridged", 5),
    ("Other Author", "Fixture Tale", 12),
    ("Fixture Author", "Asin Book", 3),
    ("Fixture Author", "Isbn Book", 3),
]


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


def download_multi_file_book(folder):
    """Multi-file MP3 book from LibriVox, no ASIN/ISBN."""
    for name in LIBRIVOX_FILES:
        download(f"https://archive.org/download/{LIBRIVOX_ID}/{name}", folder / name)
    download(f"https://archive.org/download/{LIBRIVOX_ID}/{LIBRIVOX_COVER}", folder / "cover.jpg")


def generate_multi_file_book(folder):
    """Noise in place of the LibriVox files: same names, lengths, format and tags."""
    folder.mkdir(parents=True, exist_ok=True)
    for number, (name, (title, seconds)) in enumerate(zip(LIBRIVOX_FILES, MULTI_FILE_TRACKS), 1):
        target = folder / name
        if target.exists():
            continue
        part = target.with_name(target.stem + ".part.mp3")
        ffmpeg(
            "-f", "lavfi", "-i", "anoisesrc=color=pink:amplitude=0.05:sample_rate=22050",
            "-t", str(seconds), "-ac", "1", "-c:a", "libmp3lame", "-b:a", "64k",
            "-id3v2_version", "3", "-write_id3v1", "1",
            "-metadata", f"title={number:02} - {title}", "-metadata", "artist=Oscar Wilde",
            "-metadata", f"album={MULTI_FILE_ALBUM}", "-metadata", f"track={number}",
            str(part),
        )
        part.rename(target)
    cover = folder / "cover.jpg"
    if not cover.exists():
        ffmpeg("-f", "lavfi", "-i", "testsrc2=size=300x300", "-frames:v", "1", str(cover))


def multi_file_book(real):
    """The multi-file book's folder, downloaded or generated."""
    folder = AUDIO / "Oscar Wilde" / MULTI_FILE_ALBUM
    if real:
        download_multi_file_book(folder)
    else:
        generate_multi_file_book(folder)
    return folder


def alice_ebooks(real):
    """Alice as EPUB and PDF, downloaded or generated."""
    epub = EBOOKS / "Lewis Carroll" / "Alice EPUB" / "Alice's Adventures in Wonderland.epub"
    pdf = EBOOKS / "Lewis Carroll" / "Alice PDF" / "Alice's Adventures in Wonderland.pdf"
    if real:
        download(EPUB_URL, epub)
        download(PDF_URL, pdf)
        return
    title, author = "Alice's Adventures in Wonderland", "Lewis Carroll"
    if not epub.exists():
        fake_ebooks.write_epub(epub, title, author)
    if not pdf.exists():
        fake_ebooks.write_pdf(pdf, title)


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
    shutil.copy(source_folder / "cover.jpg", target.parent / "cover.jpg")
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


def build_grid_library():
    """Many tiny items with distinct covers, for cover grid and memory tests."""
    marker = GRID / ".complete"
    if marker.exists():
        return
    work = MEDIA / ".grid"
    work.mkdir(parents=True, exist_ok=True)
    print(f"generating {GRID_ITEMS} grid items")
    ffmpeg("-f", "lavfi", "-i", "anullsrc=r=22050:cl=mono", "-t", "2", "-c:a", "libmp3lame", "-b:a", "32k", str(work / "track.mp3"))
    ffmpeg(
        "-f", "lavfi", "-i", f"testsrc2=size=600x600:rate=1:duration={GRID_ITEMS}",
        "-vf", "hue=h=t*37", "-q:v", "5", str(work / "cover_%04d.jpg"),
    )
    for n in range(1, GRID_ITEMS + 1):
        folder = GRID / f"Grid Author {n % 40:02}" / f"Grid Book {n:04}"
        folder.mkdir(parents=True, exist_ok=True)
        shutil.copy(work / "track.mp3", folder / f"Grid Book {n:04}.mp3")
        shutil.copy(work / f"cover_{n:04}.jpg", folder / "cover.jpg")
    shutil.rmtree(work)
    marker.touch()


def build_matching_library():
    """Short silent books whose metadata the init script varies per server."""
    for author, title, minutes in MATCHING_BOOKS:
        target = MATCHING / author / title / f"{title}.mp3"
        if target.exists():
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        part = target.with_name(target.stem + ".part.mp3")
        ffmpeg(
            "-f", "lavfi", "-i", "anullsrc=r=8000:cl=mono", "-t", str(minutes * 60),
            "-c:a", "libmp3lame", "-b:a", "8k", "-metadata", f"title={title}",
            "-metadata", f"artist={author}", str(part),
        )
        part.rename(target)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--real", action="store_true", help="download the public domain originals")
    args = parser.parse_args()
    source = multi_file_book(args.real)
    build_long_m4b(source)
    alice_ebooks(args.real)
    build_comic()
    build_grid_library()
    build_matching_library()
    print("fixtures ready in", MEDIA)


if __name__ == "__main__":
    main()
