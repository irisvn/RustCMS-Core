# 0008 — Media storage and rich text documents

- Status: accepted
- Date: 2026-10-14

## Context

0.2 adds uploads (local disk and S3-compatible storage) and rich text. Files
are attacker-controlled bytes served from the admin origin; rich text is
attacker-controlled markup shown in the panel, whose CSP forbids inline styles
and scripts (ADR 0006).

## Decision

- **Storage** is a narrow trait (`put_file`, `open`, `delete`, `exists`) with a
  local driver (canonical root, symlinks refused, atomic rename) and an S3
  driver (`object_store`, feature `s3`). Keys are generated, never client input.
- **Downloads always go through the app** (no presigned URLs): authorization
  (policy, scope, trash) and headers apply to every byte; access ends when a
  permission is revoked. Only png/jpeg/gif/webp are served inline; everything
  else as an attachment with `nosniff` and a sandbox CSP.
- **Types come from content** (magic bytes) and a fixed allowlist; SVG/HTML/XML
  are never accepted. Images must be decodable within a pixel budget.
- **Crash safety**: an upload intent is committed before the object is written
  and removed with the media row; a force delete queues the object's removal in
  its transaction. A worker and `media:prune` reconcile both.
- **References** (media fields, rich text images) are checked after hooks on
  every write and on restore, with the media rows locked (shared).
- **Rich text is a JSON document**, validated structurally (closed set of
  nodes, marks and attributes; bounded), rendered with React elements in the
  panel (no HTML sink) and exported as escaped HTML by `rich_text::to_html`.
  The editor (TipTap, `injectCSS: false`) passed the CSP check in the E2E suite.

## Consequences

- No public media URLs, thumbnails or range requests in 0.2: apps serve public
  files with their own routes on top of `Storage`.
- Document format changes need a new version and a migration path.

## Amendment (after 0.8.2): seeing every file without managing it

The owner scope had one way out, `media.manage_all`, which also let its
holder trash and delete other users' files (the scope opens them to every
write). `media.view_all` opens the scope only: every file is listed,
downloaded and referenced. `MediaPolicy` refuses `delete`, `restore` and
`force_delete` of a file whose `owner_id` is not the user's unless
`media.manage_all`, so the decision stays server-side on every path (the
record routes, row abilities, the trash listing). `view` stays independent of
the record (`view_depends_on_record` is false), so lists keep their single
query.
