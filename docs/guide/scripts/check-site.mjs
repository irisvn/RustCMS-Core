// After `vitepress build`: every local image exists and every link to a page
// of the guide reaches a page and, with a `#fragment`, a heading of it.
// Links outside the guide are checked separately (`just docs-links`).
//
//   node scripts/check-site.mjs [built site, default DOCS_OUT or .vitepress/dist]
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'

const here = new URL('..', import.meta.url).pathname
const out = resolve(here, process.argv[2] ?? process.env.DOCS_OUT ?? '.vitepress/dist')
const base = process.env.DOCS_BASE || '/'

function* files(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) yield* files(path)
    else if (name.endsWith('.html')) yield path
  }
}

const ids = new Map()
const idsOf = (file) => {
  if (!ids.has(file)) ids.set(file, new Set([...readFileSync(file, 'utf8').matchAll(/\sid="([^"]+)"/g)].map((m) => m[1])))
  return ids.get(file)
}

// A URL of the site → the file it serves, or null when it leaves the guide.
function target(page, url) {
  if (/^[a-z][a-z0-9+.-]*:|^\/\//i.test(url)) return null
  const path = decodeURIComponent(url)
  if (path.startsWith('/')) {
    if (!path.startsWith(base)) return null
    return join(out, path.slice(base.length))
  }
  return resolve(dirname(page), path)
}

const errors = []
let pages = 0
for (const page of files(out)) {
  pages++
  const html = readFileSync(page, 'utf8')
  const where = relative(out, page)
  for (const [, src] of html.matchAll(/<img\b[^>]*\ssrc="([^"]+)"/g)) {
    const file = target(page, src.split(/[?#]/)[0])
    if (file && !existsSync(file)) errors.push(`${where}: image ${src} is missing`)
  }
  for (const [, href] of html.matchAll(/<a\b[^>]*\shref="([^"]+)"/g)) {
    const [path, fragment] = href.split('#')
    let file = path === '' ? page : target(page, path)
    if (!file) continue
    if (file.endsWith('/')) file = join(file, 'index.html')
    if (!existsSync(file)) {
      errors.push(`${where}: link ${href} reaches no page`)
    } else if (fragment && fragment !== 'VPContent' && file.endsWith('.html') && !idsOf(file).has(decodeURIComponent(fragment))) {
      errors.push(`${where}: link ${href} reaches no heading`)
    }
  }
}
for (const page of ['index.html', '404.html', 'print.html']) {
  if (!existsSync(join(out, page))) errors.push(`${page} is missing`)
}
if (errors.length) {
  console.error(`Site check (${out}):\n  ${errors.join('\n  ')}`)
  process.exit(1)
}
console.log(`${pages} pages checked: images, links and anchors`)
