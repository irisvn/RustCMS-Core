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
]
