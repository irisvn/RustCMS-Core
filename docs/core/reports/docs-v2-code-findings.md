# Code findings from the docs v2 inventory (2026-10-28)

Found while checking the guide against the source (d4e87e3). Not fixed in the docs work: each needs the
owner's decision. The guide describes the current behaviour honestly until then.

| # | Severity | Finding | Evidence |
|---|---|---|---|
| 1 | P2 (input validation) | `SelectInput::options` / `CheckboxList` values are not checked on resource writes: `accept_input` builds its rule with `options: None`, so a client can store any value of the column's type. Action forms and `BelongsToManySelect` do check options. The guide implied options bound the value. | `crates/rustcms-admin/src/engine/adapter/input.rs:39` (vs `action_form.rs:42,70`, `many.rs:571`) |
| 2 | P3 | `app.debug` is read by no framework code (its field doc says it exposes error details in logs); `security/production.md` implies it matters. | `crates/rustcms-core/src/config.rs:49,252` |
| 3 | P3 (rule) | The hourly auth cleanup is a `tokio::spawn` loop in `AuthProvider::boot`, so it runs in every process that boots the app (`db:seed`, app commands), not only `serve`; the project rule says periodic work goes through `AppBuilder::schedule`. | `crates/rustcms-auth/src/provider.rs:203-232` |
| 4 | P3 | Several example migrations have no `use_transaction() -> Some(true)` although the guide says to keep it (demo `m20261022_000001_create_tags.rs`, `m20261101_000001_create_integrations.rs`, every LMS migration). | `examples/*/src/database/migrations/` |
| 5 | P3 (docs only) | On MySQL `xact_lock` accepts only the preseeded names `rbac`, `lineage:*`, `idem:*`; nothing tells application authors. | `crates/rustcms-core/src/db/` |
| 6 | P2 | A background export over 10,000 rows fails as `revoked` instead of a clear limit error. | `crates/rustcms-admin/src/features/exports/mod.rs:443-447` |
| 7 | P3 | The browser socket send limit is a fixed 64 KiB, independent of `[realtime] max_message_bytes`. | `admin-ui/src/lib/realtime.ts:229` |
| 8 | P3 (rule) | Media prune (and export/import prune) are `tokio::spawn` loops in `AdminProvider::boot`, not `AppBuilder::schedule`. | `crates/rustcms-admin/src/panel/provider.rs:404-440` |
| 9 | P3 | Stale comment "saves version 2" (the editor writes the lowest version a document needs). | `admin-ui/src/lib/rich-text.ts:5-6,22` |
| 10 | fixed | Stray text "Haha: " in the LMS course overview template. | `examples/lms/resources/views/admin/course_overview.html:4` |
| 11 | P3 | A handler panic is not turned into a 500 (no catch-panic layer): the connection drops. | `crates/rustcms-core/src/http/mod.rs:206-222` |
| 12 | P3 | The `make:policy` template says `record` is "the record's readable data"; it is every column, unfiltered (encrypted fields null). | `crates/rustcms-cli/templates/stubs/policy.rs.tmpl`, `crates/rustcms-admin/src/resources/policy.rs:30-33` |
| 13 | P3 | The LMS uses icons outside the icon set (`book-open`, `clipboard-check`, `graduation-cap`, `list`, `newspaper`): they fall back to the generic icon. | `examples/lms/src/admin/resources/` |
| 14 | P2 (example) | LMS dashboard: the New enrollments / New courses widgets answer 403 for the admin because `enrolled_at` and `courses.created_at` are `.guarded()` without `.readable(...)`, and widgets refuse fields the user cannot read. | `examples/lms/src/admin/resources/{course,enrollment}_resource.rs` |
| 15 | P3 (example) | The demo React page links tags to `/r/tags/${key}`; the record route is `/r/tags/records/{key}` (needs a bundle rebuild). | `examples/demo/resources/admin/src/demo/react.tsx:75` |
