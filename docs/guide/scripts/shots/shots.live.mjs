// Shots from the live apps (RBlog, RDockPanel), kept apart from shots.config.mjs.
export const LIVE = [
  // admin/theming.md: runtime branding in a real app.
  {
    id: 'admin/runtime-branding',
    site: 'rdp',
    path: '/p/settings',
    fullPage: true,
    actions: async (page) => {
      await page.getByRole('main').getByRole('tab', { name: /^Branding$/ }).click()
      await page.getByRole('main').getByText('Loading…').waitFor({ state: 'detached', timeout: 20_000 })
    },
  },
  // admin/realtime.md: a dashboard fed by an event stream (CPU, memory, network).
  { id: 'admin/realtime-rdockpanel', site: 'rdp', path: '/p/dashboard', settle: 4000 },
  // digging-deeper/scheduling.md: the Schedules page of a real app.
  { id: 'digging-deeper/schedules-rdockpanel', site: 'rdp', path: '/system/schedules' },
]
