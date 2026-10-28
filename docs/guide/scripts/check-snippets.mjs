// Every snippet import (`<<< path#region`) names a file that exists and a
// region with a start and an end marker and some code between them.
// VitePress 1.6 falls back to the whole file when a region is missing.
//
//   node scripts/check-snippets.mjs [pages directory, default src]
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'

const root = resolve(process.argv[2] ?? new URL('../src', import.meta.url).pathname)

function* pages(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) yield* pages(path)
    else if (name.endsWith('.md')) yield path
  }
}

// Same marker shapes as VitePress for `//` and `#` comments.
const marker = (kind, name) =>
  new RegExp(`^(?://|#) ?#?${kind} ${name.replace(/[-*]/g, '\\$&')}$`)

const errors = []
let count = 0
for (const page of pages(root)) {
  let fence = false
  readFileSync(page, 'utf8').split('\n').forEach((line, i) => {
    if (/^\s*(```|~~~)/.test(line)) fence = !fence
    const m = !fence && /^\s*<<<\s*(\S+)/.exec(line)
    if (!m) return
    count++
    const where = `${relative(root, page)}:${i + 1}`
    const [, raw, region] = /^([^#{[]+)(?:#([\w*-]+))?/.exec(m[1]) ?? []
    const file = raw.startsWith('@') ? join(root, raw.slice(1)) : resolve(dirname(page), raw)
    if (!existsSync(file) || !statSync(file).isFile()) return errors.push(`${where}: no file ${raw}`)
    if (!region) return errors.push(`${where}: import a #region, not a whole file`)
    const lines = readFileSync(file, 'utf8').split('\n').map((l) => l.trim())
    const starts = lines.flatMap((l, n) => (marker('region', region).test(l) ? [n] : []))
    const ends = lines.flatMap((l, n) => (marker('endregion', region).test(l) ? [n] : []))
    if (starts.length !== 1 || ends.length !== 1 || ends[0] < starts[0])
      return errors.push(`${where}: region ${region} of ${raw} needs one start and one end marker`)
    if (!lines.slice(starts[0] + 1, ends[0]).some(Boolean)) errors.push(`${where}: region ${region} is empty`)
  })
}
if (errors.length) {
  console.error(`Snippet imports:\n  ${errors.join('\n  ')}`)
  process.exit(1)
}
console.log(`${count} snippet import(s) checked`)
