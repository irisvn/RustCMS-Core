// Every package the lockfile resolves was published at least 14 days ago
// (CLAUDE.md: dependencies). Plain Node, no dependencies: it runs before
// `npm ci`, so a rejected package never runs an install script.
//
//   node scripts/check-deps-age.mjs [package-lock.json]
//
// An exception needs the owner's approval: add `name@version` to ALLOW with
// the reason.
import { readFileSync } from 'node:fs'

const ALLOW = new Set([])
const MIN_AGE_DAYS = 14

const lockPath = process.argv[2] ?? new URL('../package-lock.json', import.meta.url)
const lock = JSON.parse(readFileSync(lockPath, 'utf8'))
const wanted = new Map()
for (const [path, meta] of Object.entries(lock.packages ?? {})) {
  if (!path || meta.link || !meta.version) continue
  const name = meta.name ?? path.slice(path.lastIndexOf('node_modules/') + 'node_modules/'.length)
  wanted.set(`${name}@${meta.version}`, { name, version: meta.version })
}

const cutoff = Date.now() - MIN_AGE_DAYS * 86_400_000
const failures = []
const queue = [...wanted.values()]
async function worker() {
  for (let pkg; (pkg = queue.shift()); ) {
    const id = `${pkg.name}@${pkg.version}`
    if (ALLOW.has(id)) continue
    const res = await fetch(`https://registry.npmjs.org/${pkg.name.replace('/', '%2F')}`)
    if (!res.ok) throw new Error(`${id}: registry answered ${res.status}`)
    const published = (await res.json()).time?.[pkg.version]
    if (!published) failures.push(`${id}: no publish time`)
    else if (Date.parse(published) > cutoff) failures.push(`${id}: published ${published}`)
  }
}
await Promise.all(Array.from({ length: 8 }, worker))
if (failures.length) {
  console.error(`Packages younger than ${MIN_AGE_DAYS} days:\n  ${failures.sort().join('\n  ')}`)
  process.exit(1)
}
console.log(`${wanted.size} packages, all at least ${MIN_AGE_DAYS} days old`)
