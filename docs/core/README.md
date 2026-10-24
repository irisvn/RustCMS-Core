# RustCMS core documentation (private)

Not published. For the people building RustCMS itself.

| Document | Content |
|---|---|
| [OVERVIEW.vi.md](OVERVIEW.vi.md) | tổng quan dự án, trạng thái, lộ trình (tiếng Việt) |
| [roadmap.vi.md](roadmap.vi.md) | lộ trình: đã xong, đang hoãn (tiếng Việt) |
| [architecture.md](architecture.md) | crates, modules by role, where code lives |
| [admin-protocol.md](admin-protocol.md) | the server ↔ admin SPA contract (endpoints, schema, envelopes, idempotency, client behaviour) |
| [adr/](adr/) | architecture decision records |
| [CHANGELOG.md](CHANGELOG.md) | every change, by version |
| [plans/](plans/) | implementation plans for larger releases (historical once shipped) |
| [reports/](reports/) | báo cáo từng milestone và các đợt review (tiếng Việt) |
| [review-checklist.md](review-checklist.md) | what every review checks |
| [guides/admin-ui.md](guides/admin-ui.md) | developing the React panel (`admin-ui/`), E2E |
| [guides/releasing.md](guides/releasing.md) | tagging a release: binaries, guide and install scripts on irisvn/RustCMS |
| [guides/deployment.vi.md](guides/deployment.vi.md) | build và chạy binary (tiếng Việt; bản tiếng Anh ở guide) |

Dates in ADRs, reports and plans are milestone-plan dates, not commit
dates: the ADRs and reports of 0.3 (M13–M23) carry dates after the v0.3.0
tag (2026-10-16) that shipped them, some even later than the current release.
Git tags (`git tag --format='%(refname:short) %(creatordate:short)'`) give
the real release dates.

The public developer guide is in [`../guide/`](../guide/).
