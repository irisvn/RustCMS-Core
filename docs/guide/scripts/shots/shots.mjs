// Screenshots of the guide, from the live demo apps (read-only) or a local
// examples/demo. Never run in CI; nothing but the cropped WebP images is kept.
//
//   RDP_EMAIL=… RDP_PASSWORD=… RBLOG_EMAIL=… RBLOG_PASSWORD=… \
//     node docs/guide/scripts/shots/shots.mjs [id-prefix…]
//
// Uses the Playwright of admin-ui (PLAYWRIGHT_CHANNEL=chrome by default) and
// `cwebp`. Live demos:
// - the only write allowed is the sign-in POST (session + audit entry); the
//   exception is removed once signed in; every other non-GET/HEAD request is
//   aborted, every WebSocket closed, service workers and downloads blocked;
// - no trace, video, HAR or storage-state file; credentials only from env.
import { execFileSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { SHOTS, SITES } from './shots.config.mjs'

const repo = new URL('../../../../', import.meta.url).pathname
const { chromium } = createRequire(join(repo, 'admin-ui/package.json'))('playwright')
const out = join(repo, 'docs/guide/src/public/img')
const VIEWPORTS = { desktop: { width: 1440, height: 900 }, phone: { width: 390, height: 844 } }

const only = process.argv.slice(2)
const shots = SHOTS.filter((s) => only.length === 0 || only.some((p) => s.id.startsWith(p)))
if (shots.length === 0) throw new Error(`no shot matches ${only.join(' ')}`)

const tmp = mkdtempSync(join(tmpdir(), 'rustcms-shots-'))
const browser = await chromium.launch({ channel: process.env.PLAYWRIGHT_CHANNEL ?? 'chrome' })
const blocked = []
try {
  // One signed-in context per site, viewport and theme.
  const groups = Map.groupBy(shots, (s) => `${s.site}|${s.viewport ?? 'desktop'}|${s.theme ?? 'light'}`)
  for (const [key, group] of groups) {
    const [siteId, viewport, theme] = key.split('|')
    const site = SITES[siteId]
    if (!site) throw new Error(`unknown site ${siteId}`)
    const context = await signedIn(site, viewport, theme)
    try {
      for (const shot of group) await capture(context, site, shot)
    } finally {
      await context.close()
    }
  }
} finally {
  await browser.close()
  rmSync(tmp, { recursive: true, force: true })
}
if (blocked.length) console.log(`blocked writes (expected while opening dialogs):\n  ${blocked.join('\n  ')}`)

async function signedIn(site, viewport, theme) {
  const email = process.env[site.emailEnv]
  const password = process.env[site.passwordEnv]
  if (!email || !password) throw new Error(`set ${site.emailEnv} and ${site.passwordEnv}`)
  const context = await browser.newContext({
    viewport: VIEWPORTS[viewport],
    deviceScaleFactor: 2,
    colorScheme: theme,
    locale: 'en-US',
    extraHTTPHeaders: { 'Accept-Language': 'en' },
    serviceWorkers: 'block',
    acceptDownloads: false,
  })
  const origin = new URL(site.url).origin
  const login = `${site.url}/api/auth/login`
  let signingIn = !site.writable
  // Installed before any page exists.
  if (!site.writable) {
    await context.route('**/*', (route) => {
      const req = route.request()
      if (req.method() === 'GET' || req.method() === 'HEAD') return route.continue()
      if (signingIn && req.method() === 'POST' && req.url() === login) return route.continue()
      blocked.push(`${req.method()} ${req.url().replace(origin, '')}`)
      return route.abort('blockedbyclient')
    })
    await context.routeWebSocket(/.*/, (ws) => ws.close())
  }
  // The panel's own theme choice follows the system scheme unless one is stored.
  await context.addInitScript((scheme) => {
    try {
      localStorage.setItem('rustcms.colorScheme', scheme)
    } catch {}
  }, theme)
  const page = await context.newPage()
  await page.goto(`${site.url}/login`)
  await page.getByLabel(/^Email$/).fill(email)
  await page.getByLabel(/^(Password|Mật khẩu)$/).fill(password)
  await page.getByRole('button', { name: /^(Sign in|Đăng nhập)$/ }).click()
  await page.waitForURL((url) => !url.pathname.endsWith('/login'), { timeout: 20_000 })
  signingIn = false
  await page.close()
  return context
}

async function capture(context, site, shot) {
  const page = await context.newPage()
  try {
    await page.goto(`${site.url}${shot.path}`)
    // Live pages keep a stream open: wait for quiet, but not forever.
    await page.waitForLoadState('networkidle', { timeout: 8_000 }).catch(() => {})
    await page.getByRole('main').waitFor()
    if (shot.actions) await shot.actions(page)
    await page.waitForTimeout(shot.settle ?? 400)
    await redact(page, process.env[site.emailEnv])
    const png = join(tmp, `${shot.id}.png`)
    const mask = (shot.mask ?? []).map((m) => page.locator(m))
    const target = shot.element ? page.locator(shot.element).first() : page
    await target.screenshot({ path: png, mask, maskColor: '#d4d4d8', animations: 'disabled', fullPage: !shot.element && shot.fullPage })
    const webp = join(out, `${shot.id}.webp`)
    mkdirSync(dirname(webp), { recursive: true })
    execFileSync('cwebp', ['-quiet', '-q', '82', png, '-o', webp])
    console.log(`${shot.id}.webp  (${site.name}${shot.path})`)
  } finally {
    await page.close()
  }
}

// The signed-in account shows as "Demo User" / demo@example.com: its email, and
// the name next to it in the sidebar, are replaced in every text and title.
async function redact(page, email) {
  await page.evaluate((email) => {
    const account = document.querySelector('a[title*="@"]')
    const name = account?.querySelector('span span')?.textContent?.trim()
    const swap = (text) => {
      let out = text.split(email).join('demo@example.com')
      if (name && name.length > 1) out = out.split(name).join('Demo User')
      return out
    }
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT)
    for (let node; (node = walker.nextNode()); ) {
      const next = swap(node.nodeValue)
      if (next !== node.nodeValue) node.nodeValue = next
    }
    // The avatar's initials.
    for (const el of account?.querySelectorAll('span') ?? [])
      if (/^[A-Z0-9]{1,3}$/.test(el.textContent.trim()) && !el.children.length) el.textContent = 'DU'
    for (const el of document.querySelectorAll('[title]')) el.title = swap(el.title)
    for (const el of document.querySelectorAll('input')) if (el.value) el.value = swap(el.value)
  }, email ?? '')
}
