# 0002 — Admin UI generated from a JSON schema rendered by a React SPA

- Status: accepted
- Date: 2026-10-11

## Context

The admin UI is generated from Rust declarations, like FilamentPHP. Options were
server-rendered HTML with HTMX, a full Rust/WASM UI, or a JSON schema rendered by a SPA.

## Decision

Rust resources produce a versioned JSON schema (`schemaVersion`, `vocabularyVersion`).
A generic React 19 + Vite + shadcn/ui + TanStack SPA renders it. The built SPA ships
inside `rustcms-admin` as committed assets embedded with `rust-embed`, so applications
need no Node toolchain.

## Consequences

- The server is the only authority for validation and authorization; the schema only
  drives presentation.
- Custom field types require a rebuilt admin bundle (deferred to 0.2).
- The schema contract is tested from both sides with shared fixtures.
