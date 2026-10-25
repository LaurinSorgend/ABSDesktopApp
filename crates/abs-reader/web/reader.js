import './foliate/view.js'
import { collapse } from './foliate/epubcfi.js'
import { createTOCView } from './foliate/ui/tree.js'

const $ = selector => document.querySelector(selector)
const send = message => window.ipc.postMessage(JSON.stringify(message))

const THEMES = {
    light: { fg: '#1d1d1d', link: '#1a5fb4' },
    sepia: { fg: '#5b4636', link: '#7a4b1e' },
    dark: { fg: '#ececec', link: '#8cb4ff' },
}
const FONTS = {
    serif: 'Georgia, "Times New Roman", serif',
    sans: 'system-ui, "Segoe UI", Arial, sans-serif',
}
const WHEEL_PAUSE_MS = 250

let view
let settings
let tocView
let lastWheel = 0

const showError = error => {
    const box = $('#error')
    box.textContent = String(error?.message ?? error)
    box.hidden = false
    send({ type: 'error', message: String(error?.stack ?? error) })
}

/** foliate-js does not give pdf.js its wasm decoders; without them scans stay blank. */
const preparePdfjs = async () => {
    await import('./foliate/vendor/pdfjs/pdf.mjs')
    const lib = globalThis.pdfjsLib
    const getDocument = lib.getDocument
    const wasmUrl = new URL('pdfjs-wasm/', location.href).href
    lib.getDocument = params => getDocument({ ...params, wasmUrl })
}

const loadBook = async () => {
    const meta = await (await fetch('meta.json')).json()
    if (meta.format === 'pdf') await preparePdfjs()
    const blob = await (await fetch('book')).blob()
    return { meta, file: new File([blob], `book.${meta.format}`) }
}

const bookCSS = s => `
    @namespace epub "http://www.idpf.org/2007/ops";
    html {
        color-scheme: ${s.theme === 'dark' ? 'dark' : 'light'};
        font-size: ${s.fontSize}% !important;
    }
    html, body {
        color: ${THEMES[s.theme].fg} !important;
        background: transparent !important;
    }
    ${FONTS[s.font] ? `body, body * { font-family: ${FONTS[s.font]} !important; }` : ''}
    p, li, blockquote, dd {
        line-height: ${s.lineHeight / 100} !important;
    }
    a:link, a:visited {
        color: ${THEMES[s.theme].link} !important;
    }
    pre {
        white-space: pre-wrap !important;
    }
    aside[epub|type~="endnote"], aside[epub|type~="footnote"],
    aside[epub|type~="note"], aside[epub|type~="rearnote"] {
        display: none;
    }
`

const applySettings = next => {
    settings = next
    document.body.dataset.theme = settings.theme
    fillForm(settings)
    if (!view || view.isFixedLayout) return
    view.renderer.setAttribute('flow', settings.flow)
    view.renderer.setAttribute('gap', `${settings.margin}%`)
    view.renderer.setStyles?.(bookCSS(settings))
}

const fillForm = s => {
    const form = $('#settings')
    for (const [name, value] of Object.entries(s)) {
        const field = form.elements[name]
        if (!field) continue
        field.value = String(value)
        const output = form.querySelector(`output[for="${name}"]`)
        if (output) output.textContent = `${value}%`
    }
}

const readForm = () => {
    const form = $('#settings').elements
    return {
        flow: form.flow.value,
        theme: form.theme.value,
        font: form.font.value,
        fontSize: Number(form.fontSize.value),
        lineHeight: Number(form.lineHeight.value),
        margin: Number(form.margin.value),
    }
}

const locationOf = detail => {
    const chapter = detail.tocItem?.label ?? null
    if (view.isFixedLayout) {
        const index = detail.section?.current ?? 0
        return { page: index + 1, pages: view.book.sections.length, fraction: detail.fraction, chapter }
    }
    return { cfi: collapse(detail.cfi), fraction: detail.fraction, chapter }
}

const showPosition = (detail, location) => {
    $('#chapter').textContent = location.chapter ?? ''
    $('#position').textContent = location.page
        ? `${location.page} / ${location.pages}`
        : `${Math.round((detail.fraction ?? 0) * 100)}%`
    if (detail.tocItem?.href) tocView?.setCurrentHref?.(detail.tocItem.href)
}

const onRelocate = ({ detail }) => {
    const location = locationOf(detail)
    showPosition(detail, location)
    send({ type: 'relocate', ...location })
}

const togglePanel = (id, open) => {
    const panel = $(id)
    panel.hidden = !(open ?? panel.hidden)
}

const closePanels = () => {
    togglePanel('#toc-panel', false)
    togglePanel('#settings-panel', false)
}

const onKey = event => {
    if (event.target.closest?.('input')) return
    const k = event.key
    if (k === 'ArrowLeft' || k === 'PageUp' || (k === ' ' && event.shiftKey)) view.goLeft()
    else if (k === 'ArrowRight' || k === 'PageDown' || k === ' ') view.goRight()
    else if (k === 'Escape') closePanels()
    else if (k === 't') togglePanel('#toc-panel')
    else if (k === 's') togglePanel('#settings-panel')
    else return
    event.preventDefault()
}

const onWheel = event => {
    if (settings.flow === 'scrolled' && !view.isFixedLayout) return
    const now = Date.now()
    if (now - lastWheel < WHEEL_PAUSE_MS || Math.abs(event.deltaY) < 1) return
    lastWheel = now
    if (event.deltaY > 0) view.next()
    else view.prev()
}

const listen = target => {
    target.addEventListener('keydown', onKey)
    target.addEventListener('wheel', onWheel, { passive: true })
}

const buildToc = () => {
    const toc = view.book.toc ?? []
    tocView = createTOCView(toc, href => {
        view.goTo(href).catch(showError)
        togglePanel('#toc-panel', false)
    })
    $('#toc-view').replaceChildren(tocView.element)
    send({ type: 'toc', items: toToc(toc) })
}

const toToc = items => (items ?? []).map(item => ({
    label: item.label ?? '',
    href: item.href ?? '',
    children: toToc(item.subitems),
}))

const bindControls = () => {
    $('#toc-button').addEventListener('click', () => togglePanel('#toc-panel'))
    $('#settings-button').addEventListener('click', () => togglePanel('#settings-panel'))
    $('#prev').addEventListener('click', () => view.goLeft())
    $('#next').addEventListener('click', () => view.goRight())
    $('#book').addEventListener('click', closePanels)
    const form = $('#settings')
    form.addEventListener('input', () => applySettings(readForm()))
    form.addEventListener('change', () => send({ type: 'settings', settings: readForm() }))
    listen(document)
}

const startAt = (meta, isFixed) => {
    if (!meta.location) return null
    if (!isFixed) return meta.location.startsWith('epubcfi') ? meta.location : null
    const page = parseInt(meta.location, 10)
    return Number.isFinite(page) && page > 0 ? page - 1 : null
}

const main = async () => {
    bindControls()
    const { meta, file } = await loadBook()
    $('#title').textContent = meta.title
    view = document.createElement('foliate-view')
    $('#book').append(view)
    view.addEventListener('relocate', onRelocate)
    view.addEventListener('load', ({ detail }) => listen(detail.doc))
    await view.open(file)
    document.body.classList.toggle('fixed', view.isFixedLayout)
    applySettings(meta.settings)
    buildToc()
    const lastLocation = startAt(meta, view.isFixedLayout)
    await view.init({ lastLocation, showTextStart: lastLocation == null })
    send({ type: 'ready' })
}

window.absReader = { applySettings }
window.addEventListener('unhandledrejection', event => showError(event.reason))
main().catch(showError)
