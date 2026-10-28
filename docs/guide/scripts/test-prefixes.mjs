// Builds the guide for each published prefix into a copy of the Pages layout,
// serves it like GitHub Pages (a missing URL gets the site's root 404.html)
// and checks deep pages, their assets, the search index, the dev banner and 404.
//
//   node scripts/test-prefixes.mjs        (about 20 s)
import { execFileSync } from 'node:child_process'
import { createServer } from 'node:http'
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { extname, join } from 'node:path'
import assert from 'node:assert/strict'

const guide = new URL('..', import.meta.url).pathname
const pagesRoot = mkdtempSync(join(tmpdir(), 'rustcms-pages-'))
const site = join(pagesRoot, 'RustCMS')
const builds = [
  { base: '/RustCMS/', out: site },
  { base: '/RustCMS/v/9.9.9/', out: join(site, 'v/9.9.9') },
  { base: '/RustCMS/dev/', out: join(site, 'dev'), channel: 'dev' },
]
try {
  for (const b of builds) {
    execFileSync('npx', ['vitepress', 'build'], {
      cwd: guide,
      stdio: 'pipe',
      env: { ...process.env, DOCS_BASE: b.base, DOCS_OUT: b.out, DOCS_CHANNEL: b.channel ?? '', DOCS_VERSION: '9.9.9', DOCS_SHA: 'abcdef1234567890' },
    })
    execFileSync('node', [join(guide, 'scripts/check-site.mjs'), b.out], { stdio: 'pipe', env: { ...process.env, DOCS_BASE: b.base } })
  }

  const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json' }
  const server = createServer((req, res) => {
    const path = decodeURIComponent(new URL(req.url, 'http://x').pathname)
    let file = join(pagesRoot, path)
    if (existsSync(file) && statSync(file).isDirectory()) file = join(file, 'index.html')
    const found = file.startsWith(pagesRoot) && existsSync(file)
    res.writeHead(found ? 200 : 404, { 'content-type': types[extname(file)] ?? 'application/octet-stream' })
    res.end(readFileSync(found ? file : join(site, '404.html')))
  })
  await new Promise((r) => server.listen(0, '127.0.0.1', r))
  const origin = `http://127.0.0.1:${server.address().port}`
  const get = async (path) => {
    const res = await fetch(origin + path)
    return { status: res.status, body: await res.text() }
  }
  const assetsOf = (html) => [...html.matchAll(/(?:href|src)="(\/RustCMS\/[^"]*\/?assets\/[^"]+\.(?:js|css))"/g)].map((m) => m[1])

  try {
    for (const b of builds) {
      const page = await get(`${b.base}admin/resources.html`)
      assert.equal(page.status, 200, `${b.base} deep page`)
      const assets = assetsOf(page.body)
      assert.ok(assets.length > 0 && assets.every((a) => a.startsWith(b.base)), `${b.base} assets under the prefix`)
      for (const a of assets) assert.equal((await get(a)).status, 200, a)
      const index = readdirSync(join(b.out, 'assets/chunks')).find((f) => f.startsWith('@localSearchIndexroot.'))
      const search = await get(`${b.base}assets/chunks/${index}`)
      assert.ok(search.status === 200 && search.body.includes('scope'), `${b.base} search index`)
      assert.equal((await get(`${b.base}upgrade.html`)).status, 200, `${b.base} upgrade.html (linked by the CLI)`)
      assert.equal((await get(`${b.base}print.html`)).status, 200, `${b.base} print.html`)
      const dev = !!b.channel
      assert.equal(/name="robots" content="noindex"/.test(page.body), dev, `${b.base} robots`)
      assert.equal(page.body.includes('describes unreleased changes'), dev, `${b.base} banner`)
      assert.ok(page.body.includes(dev ? 'main (unreleased)' : 'RustCMS 9.9.9 · source abcdef123456'), `${b.base} footer`)
    }
    for (const missing of ['/RustCMS/nope.html', '/RustCMS/v/9.9.9/nope.html', '/RustCMS/dev/a/b.html']) {
      const res = await get(missing)
      assert.equal(res.status, 404, missing)
      const assets = assetsOf(res.body)
      assert.ok(assets.length > 0, `${missing}: 404 page has its assets`)
      for (const a of assets) assert.equal((await get(a)).status, 200, `${missing}: ${a}`)
      // The theme draws the page and its home link in the browser, from the root guide's base.
      assert.ok(res.body.includes('<div id="app">'), `${missing}: app shell`)
    }
  } finally {
    server.close()
  }
  console.log('prefixes /RustCMS/, /RustCMS/v/9.9.9/, /RustCMS/dev/: deep pages, assets, search, banner, 404')
} finally {
  rmSync(pagesRoot, { recursive: true, force: true })
}
