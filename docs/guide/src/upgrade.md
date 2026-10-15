# Upgrade guide

## From 0.1.0-alpha.0 to the next release

1. Update the `rustcms` dependency, then run `rustcms migrate` (new tables for
   media, JWT sign-ins, idempotency keys, locks, the audit log, settings,
   jobs, and `users.locale`). `serve` now also runs queue workers (set
   `queue.enabled = false` to keep them out of web instances).
2. Rust API changes:
   - API routes: move them to `ApiRoutes` with an explicit `Access` per route
     ([Routing](basics/routing.md)); register them with
     `AuthProvider::new().api_permissions(&api)` and `api.register(&mut app)`.
   - `RateLimiter::reserve`, `release` and `clear` are async (`.await` them).
   - `Principal` and `rustcms::models::user::Model` have new fields
     (`locale`, `credential`): struct literals need them, or `..Default::default()`.
   - `AppError::Conflict` has a new `reason` field.
   - Functions that change credentials record an audit entry and take one more
     argument: `credentials::change_password` and `credentials::logout_everywhere`
     an `&AuditContext`; `credentials::issue_token` and
     `jwt_session::{issue, rotate, logout}` a `&RequestMeta` (both in
     `rustcms::audit`; `RequestMeta` is also an extractor).
   - `InputType` has a `Secret` variant and `AdminPanel` a `settings` field
     (exhaustive matches and struct literals need them); admin clients see
     `vocabularyVersion` 5.
   - Resource slugs are limited to 64 bytes and action names to 57, and
     `user`, `role`, `permission`, `settings`, `session` are reserved (boot
     fails); `Ctx::new` records writes as its principal.
   - `ActorKind` has a `Job` variant (exhaustive matches need it).
   - `SystemLinks` has an `audit` field; resource schemas gain the
     `history` ability.
   - Tests that build tables from entities instead of running migrations also
     need the `audit_logs` table ([Testing](basics/testing.md)).
3. Optional: translate your labels. Add `lang/vi.toml`, register it with
   `AdminProvider::new().translations("vi", include_str!("../lang/vi.toml"))`,
   and fill in what `rustcms lang:missing vi` lists.
4. API clients: error bodies gain `issues` and `formIssues` (and `reason` on
   409); existing fields are unchanged.
5. Several instances: see [Several instances](digging-deeper/instances.md).
