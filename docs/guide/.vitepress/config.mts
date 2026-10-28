// The developer guide. One build per published prefix:
//   DOCS_BASE     `/RustCMS/` (latest stable), `/RustCMS/v/X.Y.Z/`, `/RustCMS/dev/`; `/` by default
//   DOCS_OUT      output directory (absolute, or relative to docs/guide), `.vitepress/dist` by default
//   DOCS_CHANNEL  `dev` for the build of `main`: banner + noindex
//   DOCS_VERSION, DOCS_SHA   shown in the footer and the version menu
import { readFileSync } from 'node:fs'
import { defineConfig } from 'vitepress'
import { sidebar } from './sidebar'

const env = process.env
const workspace = readFileSync(new URL('../../../Cargo.toml', import.meta.url), 'utf8')
const version = env.DOCS_VERSION || /^version = "([^"]+)"/m.exec(workspace)?.[1] || 'unknown'
const sha = (env.DOCS_SHA || '').slice(0, 12)
const dev = env.DOCS_CHANNEL === 'dev'
const pages = 'https://irisvn.github.io/RustCMS'

// mdBook's `normalize_id`: letters, digits, `_` and `-` kept (lowercased), whitespace becomes `-`.
function mdbookId(text: string): string {
  return [...text.trim()]
    .map((c) => (/[\p{L}\p{N}_-]/u.test(c) ? c.toLowerCase() : /\s/.test(c) ? '-' : ''))
    .join('')
}

export default defineConfig({
  title: 'RustCMS',
  description: 'Build admin panels and headless CMS backends in Rust.',
  lang: 'en',
  srcDir: 'src',
  base: env.DOCS_BASE || '/',
  outDir: env.DOCS_OUT || '.vitepress/dist',
  // Page paths stay `x.html`: links from the CLI and older pages keep working.
  cleanUrls: false,
  lastUpdated: true,
  // A dead link fails the build (links to a local server are examples).
  ignoreDeadLinks: 'localhostLinks',
  head: [
    ['meta', { name: 'rustcms-guide', content: `${version}${sha ? ` ${sha}` : ''}${dev ? ' dev' : ''}` }],
    ...(dev ? [['meta', { name: 'robots', content: 'noindex' }] as [string, Record<string, string>]] : []),
  ],
  markdown: {
    theme: { light: 'github-light', dark: 'github-dark' },
    // Heading ids as mdBook made them, so links to `page.html#heading` keep working.
    anchor: { slugify: mdbookId },
  },
  themeConfig: {
    devChannel: dev,
    siteTitle: 'RustCMS',
    nav: [
      { text: 'Guide', link: '/introduction' },
      { text: 'Reference', link: '/reference/cli' },
      {
        text: dev ? 'dev (unreleased)' : `v${version}`,
        items: [
          { text: 'Release notes', link: '/release-notes' },
          { text: 'Upgrade guide', link: '/upgrade' },
          { text: 'Latest stable guide', link: `${pages}/` },
          { text: 'Guide of main (unreleased)', link: `${pages}/dev/` },
        ],
      },
    ],
    sidebar,
    notFound: { quote: 'This page is not in the guide of this version.', linkText: 'Latest stable guide' },
    outline: { level: [2, 3] },
    search: { provider: 'local' },
    socialLinks: [{ icon: 'github', link: 'https://github.com/irisvn/RustCMS-Core' }],
    editLink: {
      pattern: 'https://github.com/irisvn/RustCMS-Core/edit/main/docs/guide/src/:path',
      text: 'Edit this page on GitHub',
    },
    footer: {
      message: `RustCMS ${dev ? 'main (unreleased)' : version}${sha ? ` · source ${sha}` : ''} · MIT OR Apache-2.0`,
    },
  },
})
