# 0004: PDF and comics in the reader process

Status: accepted

## Decision

PDF and CBZ open in `abs-reader` like EPUB, through foliate-js: PDF with the pdf.js 5.5.207 build that foliate-js vendors (`vendor/pdfjs`), CBZ with its `comic-book.js`. No pdfium, and no comic view in Slint.

## Reasons

- foliate-js already renders both formats with the same `foliate-view` element, events and keyboard handling as EPUB. One reader window, one IPC protocol, one settings panel.
- pdfium-render would need a native library per platform (about 6 MB on Windows) and a page view with zoom and scrolling written in Slint. The CBZ plan (`zip` + `image` + a page LRU) is a second viewer of the same size.
- Memory stays in the helper process and is freed when it exits, which is the Phase 3 exit criterion.
- The web client stores the same thing for both formats: `ebookLocation` is the 1-based page number and `ebookProgress` is `(page - 1) / pages` (`PdfReader.vue`, `ComicReader.vue`, identical on 2.26.3 and 2.37.2). foliate-js reports the page as the section index.

## Consequences

- pdf.js is slower than pdfium on large scanned PDFs and draws through canvas. If the 12 MB Alice scan turns out too slow to page through, revisit with pdfium in the helper rather than in the main app.
- The reader ships the foliate-js folder including `vendor/pdfjs` (13 MB with source maps, which packaging leaves out).
- CBR stays unsupported (unrar license). MOBI, AZW3 and FB2 open through foliate-js as untested extras; the web client does not save progress for MOBI, so nothing to match there.
