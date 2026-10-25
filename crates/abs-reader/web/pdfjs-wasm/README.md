The `wasm/` decoders of pdfjs-dist 5.5.207, the pdf.js version foliate-js vendors:
JPEG 2000 (`openjpeg.wasm`), JBIG2 (`jbig2.wasm`) and ICC colour (`qcms_bg.wasm`).
foliate-js leaves them out, and without them scanned PDFs render as blank pages.
Licenses are in `packaging/licenses/pdf.js-wasm-*`.

Update together with the foliate-js submodule:
`curl -L https://registry.npmjs.org/pdfjs-dist/-/pdfjs-dist-<version>.tgz | tar xz package/wasm`
