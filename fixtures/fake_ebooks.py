"""Small generated EPUB 3 and PDF files that stand in for the Alice downloads."""

import zipfile
from html import escape

EPUB_CHAPTERS = 12
PDF_PAGES = 20
PARAGRAPH = (
    "This is generated filler text for the test fixtures. It has enough words to "
    "fill a few lines, so the reader has something to lay out and paginate."
)
CONTAINER = """<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"""


def write_epub(path, title, author, cover_jpeg):
    """EPUB 3 with a JPEG cover, a nav document and EPUB_CHAPTERS chapters of filler text."""
    path.parent.mkdir(parents=True, exist_ok=True)
    chapters = [f"chapter{n:02}.xhtml" for n in range(1, EPUB_CHAPTERS + 1)]
    part = path.with_name(path.name + ".part")
    with zipfile.ZipFile(part, "w") as epub:
        epub.writestr("mimetype", "application/epub+zip", compress_type=zipfile.ZIP_STORED)
        epub.writestr("META-INF/container.xml", CONTAINER, compress_type=zipfile.ZIP_DEFLATED)
        epub.writestr("OEBPS/content.opf", _opf(title, author, chapters), compress_type=zipfile.ZIP_DEFLATED)
        epub.writestr("OEBPS/nav.xhtml", _nav(title, chapters), compress_type=zipfile.ZIP_DEFLATED)
        epub.writestr("OEBPS/cover.jpg", cover_jpeg, compress_type=zipfile.ZIP_STORED)
        for number, name in enumerate(chapters, 1):
            epub.writestr(f"OEBPS/{name}", _chapter(number), compress_type=zipfile.ZIP_DEFLATED)
    part.replace(path)


def _opf(title, author, chapters):
    items = "\n".join(
        f'    <item id="c{n}" href="{name}" media-type="application/xhtml+xml"/>'
        for n, name in enumerate(chapters, 1)
    )
    spine = "\n".join(f'    <itemref idref="c{n}"/>' for n in range(1, len(chapters) + 1))
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="id">urn:uuid:5d0f1c2e-0000-4000-8000-f1a7f1a7f1a7</dc:identifier>
    <dc:title>{escape(title)}</dc:title>
    <dc:creator>{escape(author)}</dc:creator>
    <dc:language>en</dc:language>
    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>
    <meta name="cover" content="cover"/>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="cover" href="cover.jpg" media-type="image/jpeg" properties="cover-image"/>
{items}
  </manifest>
  <spine>
{spine}
  </spine>
</package>
"""


def _nav(title, chapters):
    links = "\n".join(
        f'      <li><a href="{name}">Chapter {n}</a></li>' for n, name in enumerate(chapters, 1)
    )
    return _xhtml(escape(title), f"""<nav epub:type="toc" id="toc">
    <h1>Contents</h1>
    <ol>
{links}
    </ol>
  </nav>""")


def _chapter(number):
    paragraphs = "\n".join(f"  <p>{PARAGRAPH}</p>" for _ in range(20))
    return _xhtml(f"Chapter {number}", f"<h1>Chapter {number}</h1>\n{paragraphs}")


def _xhtml(title, body):
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="en">
<head><title>{title}</title></head>
<body>
  {body}
</body>
</html>
"""


def write_pdf(path, title):
    """PDF with PDF_PAGES pages, each showing the title and its page number."""
    path.parent.mkdir(parents=True, exist_ok=True)
    first_page = 4
    kids = " ".join(f"{first_page + 2 * i} 0 R" for i in range(PDF_PAGES))
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        f"<< /Type /Pages /Kids [{kids}] /Count {PDF_PAGES} >>".encode(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    for page in range(1, PDF_PAGES + 1):
        content_id = first_page + 2 * (page - 1) + 1
        objects.append(
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] "
            f"/Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>".encode()
        )
        objects.append(_pdf_stream(_page_text(title, page)))
    part = path.with_name(path.name + ".part")
    part.write_bytes(_pdf_file(objects))
    part.replace(path)


def _page_text(title, page):
    text = title.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")
    return (
        f"BT /F1 24 Tf 72 760 Td ({text}) Tj ET\n"
        f"BT /F1 18 Tf 72 700 Td (Page {page}) Tj ET\n"
    ).encode("latin-1")


def _pdf_stream(data):
    return b"<< /Length %d >>\nstream\n" % len(data) + data + b"\nendstream"


def _pdf_file(objects):
    """Numbers the objects from 1 and writes them with a cross-reference table."""
    out = bytearray(b"%PDF-1.4\n")
    offsets = []
    for number, body in enumerate(objects, 1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % number + body + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objects) + 1)
    out += b"".join(b"%010d 00000 n \n" % offset for offset in offsets)
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objects) + 1, xref)
    return bytes(out)
