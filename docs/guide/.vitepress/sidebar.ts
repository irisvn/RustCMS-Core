// Guide navigation, in reading order. A new page is added here.
import type { DefaultTheme } from 'vitepress'

export const sidebar: DefaultTheme.SidebarItem[] = [
  {
    text: "Start here",
    items: [
      { text: "Introduction", link: '/introduction' },
      { text: "Release notes", link: '/release-notes' },
      { text: "Upgrade guide", link: '/upgrade' },
    ],
  },
  {
    text: "Getting started",
    items: [
      { text: "Installation", link: '/getting-started/installation' },
      { text: "Configuration", link: '/getting-started/configuration' },
      { text: "Directory structure", link: '/getting-started/directory-structure' },
      { text: "Deployment", link: '/getting-started/deployment' },
    ],
  },
  {
    text: "Architecture concepts",
    items: [
      { text: "Request lifecycle", link: '/architecture/lifecycle' },
      { text: "Service providers & container", link: '/architecture/providers' },
    ],
  },
  {
    text: "The basics",
    items: [
      { text: "Routing", link: '/basics/routing' },
      { text: "Validation & pagination", link: '/basics/validation' },
      { text: "Errors", link: '/basics/errors' },
      { text: "Database & migrations", link: '/basics/database' },
      { text: "Console", link: '/basics/console' },
      { text: "Testing", link: '/basics/testing' },
    ],
  },
  {
    text: "Security",
    items: [
      { text: "Authentication", link: '/security/authentication' },
      { text: "Roles & permissions", link: '/security/authorization' },
      { text: "Credential revocation", link: '/security/revocation' },
      { text: "Production checklist", link: '/security/production' },
    ],
  },
  {
    text: "Admin panel",
    items: [
      { text: "Overview", link: '/admin/overview' },
      { text: "Dashboard", link: '/admin/dashboard' },
      { text: "Resources", link: '/admin/resources' },
      { text: "Forms", link: '/admin/forms' },
      { text: "Tables", link: '/admin/tables' },
      { text: "Actions", link: '/admin/actions' },
      { text: "Authorization", link: '/admin/authorization' },
      { text: "Relationships", link: '/admin/relationships' },
      { text: "Import and export", link: '/admin/import-export' },
      { text: "Media, uploads & rich text", link: '/admin/media' },
      { text: "Custom fields", link: '/admin/custom-fields' },
      { text: "Customizing the panel", link: '/admin/customizing' },
      { text: "Theming and branding", link: '/admin/theming' },
      { text: "React pages", link: '/admin/react-pages' },
      { text: "Admin routes", link: '/admin/admin-routes' },
      { text: "Realtime: events and WebSocket", link: '/admin/realtime' },
      { text: "Users, roles & account", link: '/admin/users-roles' },
      { text: "Languages (i18n)", link: '/admin/i18n' },
      { text: "Settings", link: '/admin/settings' },
    ],
  },
  {
    text: "APIs",
    items: [
      { text: "API authentication", link: '/api/authentication' },
    ],
  },
  {
    text: "Digging deeper",
    items: [
      { text: "Encrypted columns", link: '/digging-deeper/encryption' },
      { text: "File storage", link: '/digging-deeper/storage' },
      { text: "Several instances", link: '/digging-deeper/instances' },
      { text: "Audit log", link: '/digging-deeper/audit-log' },
      { text: "Queues", link: '/digging-deeper/queues' },
      { text: "Scheduling", link: '/digging-deeper/scheduling' },
      { text: "Mail", link: '/digging-deeper/mail' },
    ],
  },
  {
    text: "Operations",
    items: [
      { text: "Running the queue", link: '/operations/queue' },
      { text: "Retention", link: '/operations/retention' },
      { text: "Backup and restore", link: '/operations/backup-restore' },
      { text: "Rotating APP_KEY", link: '/operations/app-key' },
      { text: "Running mail", link: '/operations/mail' },
    ],
  },
  {
    text: "Examples",
    items: [
      { text: "An LMS back office", link: '/examples/lms' },
    ],
  },
  {
    text: "Reference",
    items: [
      { text: "CLI", link: '/reference/cli' },
      { text: "Database schema", link: '/reference/database-schema' },
      { text: "Admin API", link: '/reference/admin-api' },
      { text: "Error codes", link: '/reference/error-codes' },
      { text: "Wire format", link: '/reference/wire-format' },
      { text: "Limitations", link: '/reference/limitations' },
    ],
  },
]
