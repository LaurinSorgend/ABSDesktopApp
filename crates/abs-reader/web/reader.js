import './foliate/view.js'

const send = message => window.ipc.postMessage(JSON.stringify(message))

const showError = error => {
    document.getElementById('error').textContent = String(error)
    send({ type: 'error', message: String(error?.stack ?? error) })
}

const loadBook = async () => {
    const meta = await (await fetch('meta.json')).json()
    const blob = await (await fetch('book')).blob()
    return { meta, file: new File([blob], `book.${meta.format}`) }
}

const toToc = items => (items ?? []).map(item => ({
    label: item.label,
    href: item.href,
    children: toToc(item.subitems),
}))

const onKey = (view, event) => {
    if (event.key === 'ArrowLeft' || event.key === 'PageUp') view.goLeft()
    else if (event.key === 'ArrowRight' || event.key === 'PageDown' || event.key === ' ') view.goRight()
}

const main = async () => {
    const { meta, file } = await loadBook()
    const view = document.createElement('foliate-view')
    document.body.append(view)
    view.addEventListener('relocate', ({ detail }) => send({
        type: 'relocate',
        cfi: detail.cfi,
        fraction: detail.fraction,
        chapter: detail.tocItem?.label ?? null,
    }))
    await view.open(file)
    send({ type: 'toc', items: toToc(view.book.toc) })
    document.addEventListener('keydown', event => onKey(view, event))
    view.addEventListener('load', ({ detail }) =>
        detail.doc.addEventListener('keydown', event => onKey(view, event)))
    await view.init({ lastLocation: meta.location, showTextStart: !meta.location })
    send({ type: 'ready' })
}

main().catch(showError)
