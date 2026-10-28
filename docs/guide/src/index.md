---
layout: home

hero:
  name: RustCMS
  text: Admin panels and headless CMS backends in Rust
  tagline: Describe your data in Rust; get an authorized admin panel, a JSON API and the operations around them, compiled into one binary.
  actions:
    - theme: brand
      text: Get started
      link: /getting-started/installation
    - theme: alt
      text: Introduction
      link: /introduction
    - theme: alt
      text: GitHub
      link: https://github.com/irisvn/RustCMS-Core

features:
  - title: Resources, forms and tables
    details: Declare fields once; RustCMS renders the panel, validates input and serves the API.
    link: /admin/resources
  - title: Deny by default
    details: Roles and permissions, field rules and record scopes are enforced on the server for every path.
    link: /admin/authorization
  - title: Media and rich text
    details: Uploads, a media library and a validated rich-text document model.
    link: /admin/media
  - title: Queues, schedules and mail
    details: Background jobs, periodic work claimed once per slot, and mail over SMTP.
    link: /digging-deeper/queues
  - title: Extend the panel
    details: Custom pages, slots, React extensions, admin routes, events and WebSockets.
    link: /admin/customizing
  - title: SQLite, PostgreSQL, MySQL
    details: One codebase tested on the three databases, with migrations and test helpers.
    link: /basics/database
---
