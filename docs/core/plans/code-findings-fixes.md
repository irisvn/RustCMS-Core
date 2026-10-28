# Fixing the code findings of the docs v2 inventory

Status: draft r1, 2026-10-28. Source: `docs/core/reports/docs-v2-code-findings.md`. The small findings (#2, #4,
#5, #9, #10, #12, #13, #14, #15) are fixed (688765a, df9a247). This plan covers the five that change behaviour:
#1, #3 + #8, #6, #7, #11. Owner asked: fix all, test and review thoroughly, follow the project rules.

Process (fast workflow): #1 and #3/#8 touch input validation and periodic work → Codex plan review (this file),
one Codex verify after code. Gates: touched crates on SQLite while coding; `just test-matrix` (3 DBs) before
Codex verify; full gate (`just ci`, `just generated`, E2E because the SPA changes) before commit. Each part a
commit; admin assets rebuilt in a separate commit. Release version decided by the owner (behaviour changes:
0.11.0 suggested).

## Part A — #1 Select/CheckboxList options validated on resource writes

Today: `engine/adapter/input.rs:30-42` builds `InputRule { options: None }`; `accept_input` is the single path
for create (`operations.rs:416`), update (`:477`) and imports (`import.rs:84-86`, `operations.rs:488`). Action
forms, BelongsToManySelect and settings check options; resource forms do not. `option_problem`
(`validate.rs:86-88`) compares one value only.

Change:
1. `validate.rs`: `options: Option<Options<'a>>` with `enum Options { One(&[Value]), Many(&[Value]) }`.
   `One`: the value must be one of the options (`not_option`). `Many`: an array (`invalid` otherwise), every item
   an option (`not_option`), no item twice (`given_twice`); `[]` accepted; `null` handled by required/nullable
   first. All three messages already exist in `rustcms_core::i18n::TEMPLATES` and the SPA catalogs: no new
   template. Call sites (`action_form.rs:162`, `settings/http.rs:342`, `many.rs:131`) use `One`/`None`.
2. `ResourceAdapter` (`adapter/mod.rs`): at build, `options: HashMap<field, (InputType, Vec<Value>)>` from
   `form.inputs()` of kind `Select` / `CheckboxList` with non-empty static options; fields with `belongs_to`
   skipped (their key is already checked against the relation). Boot panics (deny misconfiguration early):
   - a field in an option input and in another input whose options differ (incl. a free input);
   - a `Select` option that does not decode with the column kind;
   - a `CheckboxList` on a non-JSON column.
   `Select` options stored canonical (decode → `wire::encode`), incoming values canonicalised the same way, so
   `7`/`"7"` on i32 and `"12"` on i64 compare equal. CheckboxList items compared as JSON values.
3. `input.rs` `rule()`: `options` from that map. `check_written` (hooks) unchanged: hook-written values stay
   trusted (documented).
4. The 422 is the existing shape (`errors`/`issues` with `not_option` …); imports report it per row.
5. No contract/vocabulary change (options are already in the schema; fixtures' values are valid).

Tests: unit (`validate.rs`: Many/One cases, canonicalisation); `db_test!` (create/update out of options → 422
`not_option`, `["red","pink"]`, duplicates, non-array, valid values, an update omitting the field keeps a stored
out-of-options value, a hook may write any value); imports `db_test!` (row error); boot panics
(`#[should_panic]`). Reverse check: revert the `rule()` change → the 422 tests fail.

Docs: `admin/forms.md` (replace the "not validated" warning by the rule), LMS paragraph, `basics/validation.md`,
`admin/import-export.md`, `upgrade.md` (behaviour change: such writes now 422), CHANGELOG.

Open: should a `Select` with static options on a `belongs_to` field panic at boot (today silently ignored)?
Proposed: no (keep apps booting); the relation already bounds the value.

## Part B — #3 + #8 framework cleanups become schedules

Today: `rustcms-auth/src/provider.rs:197-243` spawns an hourly auth prune (sessions, JWT, reset tokens, audit)
with a Redis lease; `rustcms-admin/src/panel/provider.rs:401-457` spawns media prune (60 s, per disk),
exports/imports prune (600 s, per disk) and idempotency prune (3600 s). They run in every process that boots
(serve, queue:work, commands, tests), against the project rule (periodic work = `AppBuilder::schedule`).

Change:
1. Core: `Schedule::framework()` (`#[doc(hidden)]`, asserts the name starts with `rustcms.`); `Schedules::add`
   accepts a reserved name only with it. Apps still cannot use `rustcms.` names by accident.
2. AuthProvider `register`: `rustcms.auth-prune` every 3600 s, timeout 1800 s, a `.task` running the four
   prunes; every part runs even if one fails; summary "N sessions, N JWT sign-ins, N links, N audit entries";
   failure → `Failure("prune_failed", "some cleanups failed (see the logs)")`. Spawn removed.
3. AdminProvider `register`: `rustcms.media-prune.<disk>` (60 s, timeout 600 s, when `.media()`),
   `rustcms.exports-prune.<disk>` (600 s, when storage is bound), `rustcms.idempotency-prune` (3600 s). Spawns
   removed; `boot` keeps only checks.
4. Slot claims (`schedule_slots`, once per slot across instances) replace the Redis leases; every prune is
   already safe when concurrent.
5. Behaviour changes (documented): cleanups run only in `serve` with `[schedule] enabled` (default), no longer in
   `queue:work`, commands or tests; slots align on the epoch; they show on System → Schedules (`schedules.manage`
   can Run now, audited) and `schedule:list`; deployments without `serve` cron `schedule:run rustcms.…` or the
   existing prune commands (unchanged). History: ~1,440 media runs/day (7-day retention).

Tests: core unit (reserved names); auth `db_test!` (`rustcms.auth-prune` registered; `schedule:run` deletes an
expired session and reset token; run recorded `succeeded` with its summary); admin `db_test!` (media/exports/
idempotency schedules registered per configuration; running the exports prune deletes an expired export; the
schedules API lists them); `rustcms-auth/tests/realtime.rs` sets `schedule.enabled = false`.

Docs: `operations/retention.md`, `digging-deeper/scheduling.md` (framework schedules, reserved prefix),
`digging-deeper/instances.md` (lease table → schedules), `reference/limitations.md`, `architecture/lifecycle.md`,
`getting-started/deployment.md`, `upgrade.md`, ADR 0031 amendment, CHANGELOG.

## Part C — #6 background export over the limit gets its own reason

Today: `features/exports/mod.rs:493` maps any validation error of `export_keys` to `revoked`.

Change: map the form issues `too_many_to_export` / `too_many_matches` (stable issue codes) to the export's
`error`, anything else stays `revoked`. `ExportView.error` doc and TS types regenerated; SPA `exports.tsx`
`reason()` + `en`/`vi` keys; admin assets rebuilt.

Tests: unit (mapping); `db_test!` with a record-dependent policy at the scan cap, or 10,001 rows on SQLite only
if fast enough; vitest for `reason()` if covered. Docs: `admin/import-export.md`, `reference/limitations.md`,
`reference/admin-api.md`, CHANGELOG.

## Part D — #7 the browser socket limit follows `[realtime] max_message_bytes`

Today: `admin-ui/src/lib/realtime.ts:224` uses 64 KiB unless the caller passes a limit; callers never do.

Change: `Realtime::max_message_bytes()` (read-only) in auth; the SPA meta config (`panel/spa.rs` `render_index`)
gets `realtime: { maxMessageBytes }`; `config.ts` accepts an integer in 1..=1 MiB; `ApiClient` keeps it;
`openSocket` uses `options.maxMessageBytes ?? api.socketMaxBytes ?? 64 KiB`. No SDK API or contract change.

Tests: `render_index` unit; admin integration (`GET /admin` meta with a configured value); vitest (config
parsing/bounds; send under/over the configured limit; explicit option wins). Docs: `admin/realtime.md`,
`getting-started/configuration.md` (keep `[realtime]` identical across instances), CHANGELOG.

## Part E — #11 a handler panic answers the standard 500

Today: no catch-panic layer (`rustcms-core/src/http/mod.rs:206-222`); hyper drops the connection.

Change: tower-http feature `catch-panic` (0.7.1 already locked; it pulls no new crate: futures-util,
http-body(-util) are already in the lock); `CatchPanicLayer::custom(panic_response)` between `with_deadline`
and `TimeoutLayer`: catches panics of routes, app middleware and guards; the 500 is `AppError::Internal`
(`{"message":"Server error.","code":"internal_error"}`, logged once in the request span, `x-request-id` kept).
A panic after a stream's head (SSE) still ends that connection; socket tasks unchanged.

Tests: `rustcms-testing/tests/http_stack.rs`: `/panic` (String and non-text payloads) → 500 JSON with
request id; panic in app middleware → 500; next request succeeds. Docs: `basics/errors.md`,
`architecture/lifecycle.md`, CHANGELOG.

## Definition of done

- Each part: tests above pass on SQLite, PostgreSQL, MySQL (`just test-matrix`); reverse checks for A and B.
- `just ci`, `just generated` clean, E2E (`PLAYWRIGHT_CHANNEL=chrome just e2e`) green; admin assets rebuilt and
  committed separately.
- Guide pages updated in the same commit as the code; CHANGELOG Unreleased; upgrade.md section.
- Codex plan review applied; one Codex verify after code, findings fixed.
- `docs-v2-code-findings.md` marks every row fixed.
