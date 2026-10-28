// What scripts/shots/shots.mjs captures. `id` is the image path under
// docs/guide/src/public/img (without .webp); `path` is relative to the site's
// admin URL; `element` crops to a selector; `mask` hides other personal data
// (the signed-in account is always renamed Demo User / demo@example.com).
// Local only: `account` signs in as another of the site's accounts, `after`
// runs once the image is saved (to undo what `actions` changed).
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
  // examples/lms run like admin-ui/e2e/serve.sh (port 8124) with its seeders; writes allowed.
  lms: {
    name: 'examples/lms',
    url: process.env.LMS_ADMIN ?? 'http://localhost:8124/admin',
    emailEnv: 'LMS_EMAIL',
    passwordEnv: 'LMS_PASSWORD',
    accounts: {
      staff: { emailEnv: 'LMS_STAFF_EMAIL', passwordEnv: 'LMS_PASSWORD' },
      teacher: { emailEnv: 'LMS_TEACHER_EMAIL', passwordEnv: 'LMS_PASSWORD' },
      editor: { emailEnv: 'LMS_EDITOR_EMAIL', passwordEnv: 'LMS_PASSWORD' },
    },
    writable: true,
  },
  // `just e2e`'s server or `cargo run -p demo -- serve` with its seeders; writes allowed.
  local: {
    name: 'examples/demo',
    url: process.env.LOCAL_ADMIN ?? 'http://localhost:8123/admin',
    emailEnv: 'LOCAL_EMAIL',
    passwordEnv: 'LOCAL_PASSWORD',
    accounts: { editor: { emailEnv: 'LOCAL_EDITOR_EMAIL', passwordEnv: 'LOCAL_EDITOR_PASSWORD' } },
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
  // security/authentication.md, security/authorization.md
  { id: 'security/sign-in', site: 'rblog', path: '/login', signedOut: true, waitFor: 'form' },
  { id: 'security/forgot-password', site: 'rblog', path: '/forgot-password', signedOut: true, waitFor: 'form' },
  { id: 'security/account', site: 'rblog', path: '/account' },
  // The editor role: its page has no form, only the permission groups.
  {
    id: 'security/roles',
    site: 'local',
    path: '/system/roles',
    fullPage: true,
    actions: async (page) => {
      await page.getByRole('main').getByRole('link', { name: /^editor$/ }).click()
      await page.getByRole('main').getByText('Resource permissions').first().waitFor()
    },
  },
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
  // admin/overview.md
  { id: 'admin/overview-shell', site: 'local', path: '' },
  { id: 'admin/overview-phone', site: 'local', path: '/r/posts', viewport: 'phone' },
  // admin/resources.md
  { id: 'admin/resources-posts-list', site: 'local', path: '/r/posts' },
  { id: 'admin/resources-record-page', site: 'local', path: '/r/posts/records/2', fullPage: true },
  // admin/tables.md
  {
    id: 'admin/tables-posts-filters',
    site: 'local',
    path: '/r/posts?filter[published][eq]=true',
    actions: async (page) => {
      await page.getByRole('main').getByRole('row').nth(1).waitFor()
    },
  },
  // admin/actions.md
  {
    id: 'admin/actions-row',
    site: 'local',
    path: '/r/posts',
    actions: async (page) => {
      await page.getByRole('main').getByRole('row').nth(1).getByRole('button', { name: /more|actions/i }).first().click()
      await page.getByRole('menu').waitFor()
    },
  },
  {
    id: 'admin/actions-bulk-bar',
    site: 'local',
    path: '/r/posts',
    actions: async (page) => {
      const rows = page.getByRole('main').getByRole('row')
      for (const n of [2, 3, 4]) await rows.nth(n).getByRole('checkbox').check()
      await page.getByRole('button', { name: /^Publish$/ }).waitFor()
    },
  },
  {
    id: 'admin/actions-form-dialog',
    site: 'local',
    path: '/r/posts',
    actions: async (page) => {
      const rows = page.getByRole('main').getByRole('row')
      for (const n of [1, 2]) await rows.nth(n).getByRole('checkbox').check()
      await page.getByRole('button', { name: /^Move to category/ }).click()
      await page.getByRole('dialog').waitFor()
    },
  },
  // admin/authorization.md: the editor's own posts.
  { id: 'admin/authorization-editor-view', site: 'local', account: 'editor', path: '/r/posts' },
  {
    id: 'admin/tables-phone',
    site: 'local',
    path: '/r/posts',
    viewport: 'phone',
    actions: async (page) => {
      await page.getByRole('main').getByRole('columnheader', { name: 'Title' }).evaluate((el) => {
        el.scrollIntoView({ block: 'start' })
        // Below the sticky top bar, with the filters' end above.
        window.scrollBy(0, -140)
      })
    },
  },
  // admin/relationships.md
  { id: 'admin/relationships-has-many', site: 'local', path: '/r/categories/records/1', fullPage: true },
  {
    id: 'admin/relationships-tags',
    site: 'local',
    path: '/r/posts/records/2',
    actions: async (page) => {
      await page.getByRole('main').getByRole('heading', { name: 'Tags' }).scrollIntoViewIfNeeded()
      await page.getByRole('main').getByRole('button', { name: /^Attach$/ }).click()
      const dialog = page.getByRole('dialog')
      await dialog.getByText('Design').click()
      await dialog.getByLabel('Position').fill('3')
    },
  },
  // admin/forms.md
  { id: 'admin/forms-editor-layout', site: 'local', path: '/r/categories/records/2/edit' },
  {
    id: 'admin/forms-post-tabs',
    site: 'local',
    path: '/r/posts/records/2/edit',
    actions: async (page) => {
      await page.getByRole('tab', { name: 'Publishing' }).click()
    },
  },
  // admin/custom-fields.md
  { id: 'admin/custom-widget', site: 'local', path: '/r/categories/records/2/edit', element: 'main form' },
  // The widget's script refused: the stored value, read-only.
  {
    id: 'admin/custom-widget-fallback',
    site: 'local',
    path: '/r/categories',
    element: 'main form',
    actions: async (page) => {
      await page.route(/demo-color/, (route) => (route.request().resourceType() === 'script' ? route.abort() : route.continue()))
      await page.goto(page.url().replace(/\/r\/categories$/, '/r/categories/records/2/edit'))
      await page.locator('main form').waitFor()
    },
  },
  // admin/customizing.md, admin/react-pages.md
  { id: 'admin/custom-page', site: 'local', path: '/p/post-stats' },
  {
    id: 'admin/script-page',
    site: 'local',
    path: '/p/live',
    settle: 2500,
    actions: async (page) => {
      await page.getByRole('textbox', { name: 'Message' }).fill('Hello from the guide')
      await page.getByRole('button', { name: 'Send' }).click()
      await page.getByText('Hello from the guide').waitFor()
    },
  },
  { id: 'admin/react-page', site: 'local', path: '/p/react-demo' },
  // digging-deeper/encryption.md
  { id: 'digging-deeper/encrypted-fields', site: 'local', path: '/r/integrations/records/1/edit' },
  // admin/admin-routes.md, admin/media.md, admin/users-roles.md: the editor role.
  {
    id: 'admin/admin-routes-roles',
    site: 'local',
    path: '/system/roles/3',
    waitFor: 'text=Resource permissions',
    actions: async (page) => {
      await page.getByRole('button', { name: /^Live/ }).click()
      await page.getByText('Use live demos').scrollIntoViewIfNeeded()
    },
  },
  {
    id: 'admin/media-roles',
    site: 'local',
    path: '/system/roles/3',
    waitFor: 'text=Resource permissions',
    element: 'fieldset:has-text("Manage every file")',
  },
  {
    id: 'admin/roles-matrix',
    site: 'local',
    path: '/system/roles/3',
    waitFor: 'text=Resource permissions',
    actions: async (page) => {
      await page.getByRole('button', { name: 'Start from preset' }).click()
    },
  },
  // admin/media.md
  { id: 'admin/media-library', site: 'local', path: '/r/media' },
  // Mountain lake (uploaded for the guide): its file, details and form.
  {
    id: 'admin/media-record',
    site: 'local',
    path: '/r/media',
    actions: async (page) => {
      const key = await page.evaluate(async () => {
        const items = (await (await fetch('/admin/api/resources/media/records?search=Mountain')).json()).data
        return items.find((m) => m.title === 'Mountain lake').key
      })
      await page.goto(`${page.url()}/records/${key}`)
      await page.getByText('Alternative text').waitFor()
    },
  },
  {
    id: 'admin/media-picker',
    site: 'local',
    path: '/r/posts/records/6/edit',
    actions: async (page) => {
      await page.getByRole('main').getByRole('button', { name: /^Media library$/ }).first().click()
      await page.getByRole('dialog').waitFor()
    },
  },
  { id: 'admin/media-unavailable', site: 'local', path: '/r/posts/records/4', waitFor: 'text=File unavailable' },
  {
    id: 'admin/rich-text-editor',
    site: 'local',
    path: '/r/posts/records/2/edit',
    element: '[role="toolbar"] >> xpath=..',
    settle: 1500,
  },
  // admin/import-export.md
  {
    id: 'admin/export-list',
    site: 'local',
    path: '/r/posts?per_page=10',
    fullPage: true,
    actions: async (page) => {
      // The panel offers a background export only past the inline limit: ask for one directly.
      await page.evaluate(async () => {
        const { csrfToken } = await (await fetch('/admin/api/auth/csrf')).json()
        const headers = { 'X-CSRF-Token': csrfToken, 'Content-Type': 'application/json' }
        await fetch('/admin/api/resources/posts/exports?filter%5Bpublished%5D%5Beq%5D=true', { method: 'POST', headers, body: '{}' })
      })
      await page.reload()
      await page.getByText('Exports', { exact: true }).waitFor()
    },
    settle: 2500,
  },
  // admin/users-roles.md
  {
    id: 'admin/users-invite',
    site: 'local',
    path: '/system/users/create',
    actions: async (page) => {
      await page.getByLabel('Name').fill('Grace Writer')
      await page.getByLabel('Email').fill('grace@example.test')
      await page.getByLabel('Send an invitation instead of setting a password').check()
      await page.getByText('editor', { exact: true }).click()
    },
  },
  // After an invitation (users-invite saves one when run with `invite`).
  {
    id: 'admin/users-list',
    site: 'local',
    path: '/system/users/create',
    actions: async (page) => {
      if (await page.evaluate(async () => (await (await fetch('/admin/api/system/users?search=grace')).text()).includes('grace@'))) {
        await page.goto(page.url().replace(/\/create$/, ''))
        return
      }
      await page.getByLabel('Name').fill('Grace Writer')
      await page.getByLabel('Email').fill('grace@example.test')
      await page.getByLabel('Send an invitation instead of setting a password').check()
      await page.getByText('editor', { exact: true }).click()
      await page.getByRole('button', { name: /^(Send the invitation|Create user)$/ }).click()
      await page.waitForURL((url) => !url.pathname.endsWith('/create'))
      await page.goto(page.url().replace(/\/system\/users.*$/, '/system/users'))
      await page.getByText('Grace Writer').waitFor()
    },
  },
  { id: 'admin/account', site: 'local', path: '/account' },
  // admin/settings.md
  // A logo picked from the library (not saved).
  {
    id: 'admin/settings-site',
    site: 'local',
    path: '/settings/site',
    fullPage: true,
    actions: async (page) => {
      await page.getByRole('main').getByRole('button', { name: /^Media library$/ }).click()
      const dialog = page.getByRole('dialog')
      await dialog.getByText('Mountain lake').click()
      await dialog.getByRole('button', { name: /^Use$/ }).click()
      await dialog.waitFor({ state: 'hidden' })
    },
  },
  // The mail password set, then Replace: the new value, Clear and Undo.
  {
    id: 'admin/settings-secret',
    site: 'local',
    path: '/settings/site',
    element: 'main form',
    actions: async (page) => {
      await page.getByRole('main').getByRole('button', { name: 'Replace' }).click()
    },
  },
  // Saved meanwhile by another tab: the tagline changes underneath, then this page saves.
  {
    id: 'admin/settings-stale',
    site: 'local',
    path: '/settings/site',
    actions: async (page) => {
      await page.evaluate(async () => {
        const { csrfToken } = await (await fetch('/admin/api/auth/csrf')).json()
        const current = await (await fetch('/admin/api/settings/site')).json()
        await fetch('/admin/api/settings/site', {
          method: 'PUT',
          headers: { 'X-CSRF-Token': csrfToken, 'Content-Type': 'application/json' },
          body: JSON.stringify({ version: current.version, data: { tagline: `Saved in another tab ${Date.now()}` } }),
        })
      })
      await page.getByLabel('Tagline').fill('News and guides')
      await page.getByRole('button', { name: 'Save settings' }).click()
      await page.getByText('Someone else saved these settings meanwhile.').waitFor()
    },
  },
  { id: 'admin/settings-mail', site: 'local', path: '/settings/mail', fullPage: true },
  // admin/realtime.md
  {
    id: 'admin/realtime-live',
    site: 'local',
    path: '/p/live',
    settle: 2500,
    actions: async (page) => {
      await page.getByRole('textbox', { name: 'Message' }).fill('Hello from the guide')
      await page.getByRole('button', { name: 'Send' }).click()
      await page.getByText('Hello from the guide').waitFor()
    },
  },
  // digging-deeper
  { id: 'digging-deeper/schedules-page', site: 'local', path: '/system/schedules' },
  // Needs a failed job: run with LOCAL_ADMIN pointing at a demo whose SMTP port is closed,
  // after a password reset request (5 attempts, about 3 minutes).
  { id: 'digging-deeper/queue-page', site: 'local', path: '/system/queue' },
  // operations/mail.md: run with LOCAL_ADMIN pointing at a demo whose SMTP port is closed
  // (smtp_connect, shown as its message).
  {
    id: 'operations/mail-test',
    site: 'local',
    path: '/settings/mail',
    element: 'div.rounded-xl:has(h2:text-is("Test mail"))',
    actions: async (page) => {
      await page.getByLabel('Recipient address').fill('someone@example.test')
      await page.getByRole('button', { name: 'Send test mail' }).click()
      await page.getByRole('main').getByRole('alert').waitFor()
    },
  },
  // admin/import-export.md: a CSV file mapped and checked (one row without a name), not imported.
  {
    id: 'admin/import-dialog',
    site: 'local',
    path: '/r/tags/import',
    fullPage: true,
    actions: async (page) => {
      const csv = 'name,notes\nTesting,QA topics\nPerformance,Speed and memory\n,Missing name\nRust,Already there\n'
      await page.getByLabel('CSV file').setInputFiles({ name: 'tags.csv', mimeType: 'text/csv', buffer: Buffer.from(csv) })
      await page.getByRole('button', { name: /^Upload$/ }).click().catch(() => {})
      await page.getByRole('button', { name: /^Check$/ }).click()
      await page.getByText(/rows can be imported/).waitFor()
    },
    // Remove the imported tags.
    after: async (page) => {
      await page.evaluate(async () => {
        const { csrfToken } = await (await fetch('/admin/api/auth/csrf')).json()
        const tags = (await (await fetch('/admin/api/resources/tags/records?per_page=100')).json()).data
        for (const tag of tags.filter((t) => ['Testing', 'Performance'].includes(t.title)))
          await fetch(`/admin/api/resources/tags/records/${tag.key}`, { method: 'DELETE', headers: { 'X-CSRF-Token': csrfToken } })
      })
    },
  },
  // admin/i18n.md: Vietnamese, chosen on the Account page, switched back to English after each.
  {
    id: 'admin/i18n-account',
    site: 'local',
    path: '/account',
    actions: async (page) => {
      await page.getByLabel('Language').selectOption('vi')
      await page.getByText('Tài khoản của bạn').first().waitFor().catch(() => {})
    },
    after: english,
  },
  {
    id: 'admin/i18n-vietnamese',
    site: 'local',
    path: '/account',
    actions: async (page) => {
      await page.getByLabel('Language').selectOption('vi')
      await page.goto(page.url().replace(/\/account$/, '/r/posts'))
      await page.getByRole('main').getByRole('row').nth(1).waitFor()
    },
    after: english,
  },
  {
    id: 'admin/i18n-validation',
    site: 'local',
    path: '/account',
    actions: async (page) => {
      await page.getByLabel('Language').selectOption('vi')
      await page.goto(page.url().replace(/\/account$/, '/r/categories/create'))
      await page.getByRole('main').locator('form').waitFor()
      // The slug filled, the name left empty: the required message.
      await page.getByRole('main').getByRole('textbox').nth(1).fill('tin-tuc')
      await page.getByRole('main').locator('form button[type=submit]').first().click()
      await page.getByRole('main').locator('form').getByText(/không được để trống/).first().waitFor()
    },
    after: english,
  },
  // examples/lms (enrollment progress and a withdrawal set up for the shots; lessons moved).
  { id: 'admin/slot', site: 'lms', path: '/r/courses/records/1', waitFor: 'text=Average progress' },
  { id: 'admin/custom-cell', site: 'lms', path: '/r/enrollments', settle: 1000 },
  { id: 'admin/theme-sign-in', site: 'lms', path: '/login', signedOut: true, waitFor: 'form' },
  { id: 'admin/theme-scheme-switch', site: 'lms', path: '', element: '[role=group][aria-label="Color scheme"] >> xpath=..' },
  // Light and dark side by side: two shots of the overview, composed on a blank page.
  {
    id: 'admin/theme-light-dark',
    site: 'lms',
    path: '/r/courses',
    element: '#pair',
    actions: async (page) => {
      const light = (await page.screenshot()).toString('base64')
      await page.getByRole('group', { name: 'Color scheme' }).getByRole('button', { name: 'Dark' }).click()
      await page.waitForTimeout(400)
      const dark = (await page.screenshot()).toString('base64')
      // Back to the system scheme (stored in this browser only).
      await page.getByRole('group', { name: 'Color scheme' }).getByRole('button', { name: 'System' }).click()
      await page.goto('about:blank')
      await page.setContent(
        `<body style="margin:0;background:#e4e4e7"><div id="pair" style="display:flex;gap:16px;padding:16px;width:max-content">` +
          `<img style="width:712px;border-radius:8px" src="data:image/png;base64,${light}">` +
          `<img style="width:712px;border-radius:8px" src="data:image/png;base64,${dark}"></div></body>`,
      )
    },
  },
  // admin/dashboard.md, examples/lms.md (enrollments spread over the last weeks for the shots).
  { id: 'admin/dashboard', site: 'lms', path: '', waitFor: 'text=Enrollments per week', settle: 1500 },
  { id: 'examples/lms-dashboard', site: 'lms', account: 'staff', path: '', waitFor: 'text=Enrollments per week', settle: 1500 },
  { id: 'examples/lms-courses-staff', site: 'lms', account: 'staff', path: '/r/courses' },
  { id: 'examples/lms-courses-teacher', site: 'lms', account: 'teacher', path: '/r/courses' },
  // Databases with SeaORM, its capacity set to its active enrollments.
  {
    id: 'examples/lms-enroll-no-seat',
    site: 'lms',
    account: 'staff',
    path: '/r/courses/records/3',
    actions: async (page) => {
      await page.getByRole('main').getByRole('button', { name: /^More actions for Databases/ }).click()
      await page.getByRole('menuitem', { name: /^Enroll a learner/ }).click()
      const dialog = page.getByRole('dialog')
      await dialog.getByRole('combobox', { name: 'Learner' }).click()
      // A learner not enrolled in the course.
      await page.getByRole('option').first().click()
      await dialog.getByRole('button', { name: /^Enroll a learner$/ }).click()
      await dialog.getByText(/no seats left/).waitFor()
    },
  },
  {
    id: 'examples/lms-lessons-move',
    site: 'lms',
    account: 'staff',
    path: '/r/courses/records/1',
    actions: async (page) => {
      await page.getByRole('main').getByRole('button', { name: /^More actions for Lesson 2$/ }).click()
      await page.getByRole('menu').waitFor()
    },
  },
  { id: 'examples/lms-news-editor', site: 'lms', account: 'editor', path: '/r/news/records/1', fullPage: true, settle: 1000 },
  { id: 'examples/lms-course-overview', site: 'lms', account: 'staff', path: '/p/course-overview' },
  { id: 'examples/lms-seats-slot', site: 'lms', account: 'staff', path: '/r/courses/records/1', waitFor: 'text=Average progress' },
  { id: 'examples/lms-progress-cell', site: 'lms', account: 'staff', path: '/r/enrollments', settle: 1000 },
  {
    id: 'digging-deeper/audit-log',
    site: 'lms',
    path: '/system/audit',
    actions: async (page) => {
      await page.getByLabel('Subject type').fill('lessons')
      await page.getByRole('button', { name: /^Filter$/ }).click()
      await page.getByRole('main').getByText('Action move_up').first().waitFor()
      await page.getByRole('main').getByText('Updated').first().click()
    },
  },
  {
    id: 'digging-deeper/audit-history',
    site: 'lms',
    path: '/r/lessons/records/3',
    actions: async (page) => {
      await page.getByRole('button', { name: 'Show history' }).click()
      await page.getByRole('main').getByText('Updated').first().click()
      await page.getByRole('heading', { name: 'History' }).scrollIntoViewIfNeeded()
    },
  },
]

// Back to English on the account (the language is saved on the server).
async function english(page) {
  await page.evaluate(async () => {
    const { csrfToken } = await (await fetch('/admin/api/auth/csrf')).json()
    await fetch('/admin/api/auth/locale', {
      method: 'POST',
      headers: { 'X-CSRF-Token': csrfToken, 'Content-Type': 'application/json' },
      body: JSON.stringify({ locale: 'en' }),
    })
  })
}
