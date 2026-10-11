# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- M0: workspace, CI (lint, features, tests on SQLite + PostgreSQL, docs, cargo-deny).
- `rustcms-core`: `AppError` with Laravel-compatible JSON errors, `Principal` with
  super-admin bypass bounded by token abilities.
- `rustcms-admin`: strict wire format codec, `Resource` trait, type-erased generic CRUD
  engine (key strategies, field permissions, scope, relationships, actions, hooks in
  transactions, constraint mapping, request limits).
- Docs: mdBook skeleton, admin protocol, ADR 0001–0003.
