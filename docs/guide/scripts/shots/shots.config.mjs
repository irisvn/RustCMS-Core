// What scripts/shots/shots.mjs captures. `id` is the image path under
// docs/guide/src/public/img (without .webp); `path` is relative to the site's
// admin URL; `element` crops to a selector; `mask` hides other personal data
// (the signed-in account is always renamed Demo User / demo@example.com).
export const SITES = {
  rblog: {
    name: 'RBlog',
    url: 'https://cms.ducdoblog.com/admin',
    emailEnv: 'RBLOG_EMAIL',
    passwordEnv: 'RBLOG_PASSWORD',
  },
  rdp: {
    name: 'RDockPanel',
    url: 'https://rdockpanel.ducdoblog.com/admin',
    emailEnv: 'RDP_EMAIL',
    passwordEnv: 'RDP_PASSWORD',
  },
  // `just e2e`'s server or `cargo run -p demo -- serve` with its seeders; writes allowed.
  local: {
    name: 'examples/demo',
    url: process.env.LOCAL_ADMIN ?? 'http://localhost:8123/admin',
    emailEnv: 'LOCAL_EMAIL',
    passwordEnv: 'LOCAL_PASSWORD',
    writable: true,
  },
}

export const SHOTS = [
  // introduction.md
  { id: 'introduction/posts-list', site: 'rblog', path: '/r/posts' },
  {
    id: 'introduction/post-form',
    site: 'rblog',
    path: '/r/posts',
    actions: async (page) => {
      await page.getByRole('main').getByRole('row').nth(1).getByRole('link').first().click()
      await page.getByRole('main').getByRole('link', { name: /^Edit$/ }).first().click()
      await page.getByRole('main').locator('form').first().waitFor()
    },
  },
  { id: 'introduction/dashboard', site: 'local', path: '' },
  // getting-started/installation.md
  { id: 'getting-started/sign-in', site: 'local', path: '/login', signedOut: true, waitFor: 'form' },
  // basics/errors.md: a 422 shown in the form (empty title).
  {
    id: 'basics/form-errors',
    site: 'local',
    path: '/r/posts/create',
    actions: async (page) => {
      await page.getByRole('main').getByRole('button', { name: /^(Create|Save)$/ }).first().click()
      await page.getByText(/required|must/i).first().waitFor()
    },
  },
]
