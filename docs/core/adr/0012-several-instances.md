# 0012 — Several instances (Redis)

- Status: accepted
- Date: 2026-10-15

## Context

0.1 ran one instance: rate limits and the permission cache lived in memory,
and periodic jobs ran in every process. 0.2 supports several instances of one
application.

## Decision

- **Permissions need no Redis.** The cache is keyed by a counter in the
  database (`auth_generations.rbac`), advanced inside every RBAC transaction
  and read on every request: a committed change applies on every instance from
  its next request, an uncommitted one on none. A Redis-based invalidation
  could commit in the database and fail in Redis, serving stale permissions.
- **Rate limits** use Redis when configured (feature `redis`): one Lua script
  checks every key, then counts all of them (all-or-nothing, as in memory);
  windows start when their key is created and refunds never extend them or go
  below zero. If Redis does not answer, limits fail closed.
- **Periodic jobs** take a Redis lease (random owner token, compare-and-delete
  release) so two instances never run one job at the same time (each may still
  run it in its own interval). Leases only avoid duplicate work:
  every job stays correct when two run, e.g. media deletions are claimed with
  `FOR UPDATE SKIP LOCKED` on PostgreSQL and MySQL.
- **Scope:** one standalone Redis (no Cluster), one key prefix per deployment;
  connect and command timeouts bounded. Password-hashing and upload concurrency
  limits stay per instance (they bound that instance's CPU and disk).

## Consequences

- One extra primary-key read per request for the permission generation.
- Without Redis, several instances work but each keeps its own rate limits:
  run one instance unless Redis is configured.
- Media storage must be shared between instances (S3 or one volume).
