# 0001 — Generic CRUD over SeaORM via type-erased resources

- Status: accepted (M0 go)
- Date: 2026-10-11

## Context

Applications declare resources over their own SeaORM entities. The HTTP layer
must serve every resource through the same routes without generics in handlers.

## Decision

- `Resource` is implemented per entity. `ResourceAdapter<R>` implements the
  generic engine once per resource and is erased to `Arc<dyn DynResource>`,
  looked up by slug in a `Registry`.
- SeaORM is pinned to `=2.0.4`. All SeaORM-specific calls live in `rustcms-admin`'s adapter.
- Writes go through `ActiveModelTrait::try_set` with values decoded by the wire codec
  (ADR 0003), never through `from_json`. `try_set` fails instead of panicking on a
  type mismatch; ambiguous column types (e.g. `timestamptz` as `DateTime<Utc>` or
  `DateTime<FixedOffset>`) are resolved by trying candidates in order.
- Columns are resolved by iterating `Entity::Column` at registration. Requests can only
  name declared fields (whitelist); misconfiguration panics at boot, not per request.
- Relationships between resources go through `DynResource::titles` and
  `keys_in_scope`, so the target's scope and policy always apply.

## Consequences

- Composite primary keys are rejected at registration (0.1).
- `ConnectionTrait` is not dyn-compatible: helpers are generic over the connection,
  and relationship checks inside writes receive the `DatabaseTransaction`.
- Action closures need an explicit `Vec<Model>` annotation (HRTB inference); revisit
  in M3 with a typed builder.

## 0.2

MySQL is the third database (ADR 0010); the engine has no MySQL-specific code
beyond search SQL, point row locks and constraint-name mapping.

## M0 evidence

`crates/rustcms-admin/tests/engine.rs`: 10 scenarios × {SQLite file, PostgreSQL 17},
covering the three key strategies, hooks in transactions with rollback, field
permissions, scope, relationship scope, per-record bulk actions, filter whitelisting
and bounds, constraint mapping, deny by default.
