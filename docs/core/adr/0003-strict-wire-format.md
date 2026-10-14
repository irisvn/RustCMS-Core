# 0003 — Strict wire format for 64-bit integers, decimals and time

- Status: accepted
- Date: 2026-10-11

## Context

JavaScript numbers cannot represent all `i64` values or exact decimals. Date-time
columns differ between "local" values and instants. SQLite stores decimals as REAL.

## Decision

- `bigint` and decimals travel as strings; decimals are rendered with the column's
  declared scale so output is identical on every database.
- Three time kinds: `date`, `local_datetime` (no timezone), `instant` (RFC 3339,
  rendered in UTC).
- Decoding is strict: the wrong JSON shape is a 422, never a coercion.

## Consequences

- The SPA must keep `I64`/`Decimal` values as strings and use a decimal-aware input.
- SQLite decimal precision is limited to `f64`; documented in the support boundary.
