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

// VitePress 1.6.4's `findRegion` (node/chunk-*.js), ported as is: the first start
// marker of any known syntax, then the first end marker of the same syntax.
const REGION = [
  /^\/\/ ?#?((?:end)?region) ([\w*-]+)$/,
  /^\/\* ?#((?:end)?region) ([\w*-]+) ?\*\/$/,
  /^#pragma ((?:end)?region) ([\w*-]+)$/,
  /^<!-- #?((?:end)?region) ([\w*-]+) -->$/,
  /^#((?:End )Region) ([\w*-]+)$/,
  /^::#((?:end)region) ([\w*-]+)$/,
  /^# ?((?:end)?region) ([\w*-]+)$/,
]
function isMarker(line, regexp, name, end) {
  const [full, tag, found] = regexp.exec(line.trim()) || []
  return full && tag && found === name && tag.match(end ? /^[Ee]nd ?[rR]egion$/ : /^[rR]egion$/)
}
function findRegion(lines, name) {
  let regexp = null
  let start = -1
  for (const [i, line] of lines.entries()) {
    if (regexp === null) {
      const r = REGION.find((reg) => isMarker(line, reg, name, false))
      if (r) [regexp, start] = [r, i + 1]
    } else if (isMarker(line, regexp, name, true)) {
      return lines.slice(start, i).filter((l) => !regexp.test(l.trim()))
    }
  }
  return null
}

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
    const code = findRegion(readFileSync(file, 'utf8').replace(/\r\n/g, '\n').split('\n'), region)
    if (code === null) return errors.push(`${where}: no region ${region} in ${raw} (VitePress would show the whole file)`)
    if (!code.some((l) => l.trim())) errors.push(`${where}: region ${region} is empty`)
  })
}
if (errors.length) {
  console.error(`Snippet imports:\n  ${errors.join('\n  ')}`)
  process.exit(1)
}
console.log(`${count} snippet import(s) checked`)
