// The checks fail on broken input (fixtures in scripts/fixtures).
import { execFileSync } from 'node:child_process'
import assert from 'node:assert/strict'

const run = (script, arg) => {
  try {
    execFileSync('node', [new URL(script, import.meta.url).pathname, arg], { stdio: 'pipe', env: { ...process.env, DOCS_BASE: '/' } })
    return ''
  } catch (e) {
    return String(e.stderr)
  }
}
const fixture = (name) => new URL(`fixtures/${name}`, import.meta.url).pathname

const snippets = run('check-snippets.mjs', fixture('snippets'))
assert.match(snippets, /page\.md:3: region missing/)
assert.match(snippets, /page\.md:5: import a #region/)
assert.match(snippets, /page\.md:7: no file/)
assert.doesNotMatch(snippets, /inside-a-fence/)

const site = run('check-site.mjs', fixture('site'))
assert.match(site, /link \.\/a\/page\.html#nowhere reaches no heading/)
assert.match(site, /link \.\/missing\.html reaches no page/)
assert.match(site, /image \/img\/missing\.webp is missing/)
assert.match(site, /404\.html is missing/)
assert.doesNotMatch(site, /#there|example\.com/)
console.log('checks fail on broken input')
