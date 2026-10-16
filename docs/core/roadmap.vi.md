# Roadmap RustCMS

> Cập nhật: 15/10/2026. Trạng thái: ✅ xong (đã commit) · 🔧 đang làm · ⏳ chưa làm.
> Plan chi tiết từng milestone: `~/.claude/plans/rustcms-*.md`; báo cáo: `docs/core/reports/`.

## 0.1 — Nền tảng (xong)
| # | Nội dung | Trạng thái |
|---|---|---|
| M0 | Workspace, spike CRUD engine trên SeaORM, CI | ✅ `c0ad4ff` |
| M1 | Core: config, provider, container, HTTP, migration, console, testing kit, CLI, app demo | ✅ `61342b0` |
| M2 | Xác thực (session, API token), role & permission theo guard | ✅ `ff326fd` |
| M3 | Resource engine, admin JSON API, phân quyền field/record, scope | ✅ `aa55f56` … `cef1c69` |
| M4 | Admin SPA (React) nhúng trong binary | ✅ `9b161aa` |
| M5 | Trang Users, Roles, Account | ✅ `6b982b7` |
| M6 | Trải nghiệm developer, chuẩn bị phát hành | ✅ `48910c0` |

## 0.2 — Mở rộng (xong)
| # | Nội dung | Trạng thái |
|---|---|---|
| M7 | Soft delete, idempotency key, Tabs, KeyValue | ✅ `3ede8d9` |
| M8 | Thư viện media, upload file (local/S3), rich text | ✅ `dc9d401` |
| M9 | Quản lý quan hệ has-many, widget field tuỳ biến | ✅ `b3fbf61` |
| M10 | MySQL 8.4 (DB thứ ba) | ✅ `8917a33` |
| R1 | Tổ chức lại module theo vai trò, tách file lớn | ✅ `acb025a` |
| R2 | Tầng API ứng dụng: `ApiRoutes`, `Access`, `Pagination`, `Validated`, `make:api` | ✅ `f59dbb3` |
| M11 | JWT + refresh token xoay vòng, Redis, chạy nhiều instance | ✅ `768b9aa` |

## 0.3 — "LMS-ready admin" (đang làm)
Mục tiêu: đủ để dựng dashboard quản trị nội bộ cho hệ thống LMS (role & permission theo resource,
settings, audit log, i18n, media S3/R2, tin tức, nhiều-nhiều, import/export, dashboard, mail).

| # | Nội dung | Trạng thái |
|---|---|---|
| M12 | Giao diện admin tiếng Anh + tiếng Việt (catalog, mã lỗi theo field, chọn ngôn ngữ theo user) | ✅ `737823f` |
| — | Tách docs: guide public (EN) / core private | ✅ `c7533c3` |
| M13 | Audit log: ghi trong transaction, actor/request, che dữ liệu, retention | ✅ `c7533c3` |
| M14a | Trang Settings theo nhóm, secret mã hoá AES-GCM | ✅ `7d1ee2d` |
| M14b | Xem audit log (kiểm toán viên) + tab Lịch sử trên bản ghi | ✅ `0750544` |
| M15a | Queue trong DB: dispatch trong transaction, worker có lease/heartbeat/timeout, retry, dừng êm | ✅ `17ad424` |
| M15b | Kiểm thật S3-compatible (S3Mock), cấu hình Cloudflare R2, timeout/streaming | ✅ `4fd54a0` + fix |
| R3 | Cấu trúc app kiểu Laravel trong `src/` (bootstrap, providers, routes, http, services, jobs, settings, admin/{resources,pages}, database/{migrations,seeders,factories}), `db:seed`, generator mới, `lock_parent` | ✅ `ac50ab5` + fix |
| M15c | `examples/lms` khung tối thiểu: khoá học, bài học, học viên, ghi danh (sĩ số, instructor) — viết lại theo R3 | ✅ `ac50ab5` + fix |
| M15d | Trang Queue trong Admin (System → Queue): tổng quan, danh sách job, retry/forget/cancel, `queue.view`/`queue.manage` | ✅ `111f25a` + fix |
| M15e | Tuỳ biến Admin: Theme API, trang/slot tuỳ biến (template HTML `resources/views/admin` hoặc script), `registerCell`, `make:page`; thư mục `resources/` | ✅ `68a5366` + fix |
| M16 | Quan hệ nhiều-nhiều (attach/detach/pivot) + action có form (modal) + ma trận quyền theo resource | ✅ `75aeab8` + fix |
| M17 | Import / Export CSV (chạy nền qua queue, kiểm quyền mỗi lô, chống CSV injection) | ✅ `9ea0531` + M17b `f1aaa6f` + fix |
| M18 | Excel (XLSX): giới hạn zip bomb, kiểu dữ liệu tường minh | ⏸ tạm bỏ qua (22/10/2026), làm sau 0.3 nếu cần |
| M19 | Mail: SMTP qua settings, gửi qua queue | ✅ |
| M20 | Quên mật khẩu + mời user + ghi nhớ đăng nhập (token một lần, revoke trong cùng transaction) | ✅ |
| M21 | Media: Range/video, thumbnail (giới hạn decode an toàn) | ⏸ tạm bỏ qua (22/10/2026), làm sau 0.3 nếu cần |
| M22 | Dashboard widgets dựng sẵn (đếm/nhóm theo quyền, không cache), dùng cơ chế slot của M15e | ✅ |
| M23 | Hoàn thiện `examples/lms`, docs vận hành, tutorial tiếng Việt, tag `v0.3.0` | ⏳ |

## Sau 0.3 (đã thống nhất hướng)
- M18 Excel (XLSX) và M21 Media Range/video, thumbnail: anh quyết định tạm bỏ qua ngày 22/10/2026.
- `admin:eject` (chép mã nguồn SPA vào app) và tài liệu dùng Admin API kiểu headless.

## Ngoài phạm vi 0.3
Nội dung đa ngôn ngữ (translatable fields), plugin/marketplace, website public / headless pages & menus,
SSO/OAuth, 2FA, thông báo realtime trong app, video transcoding, quiz và chấm điểm tự động, URL media
public có chữ ký.

## Quy trình mỗi milestone
Plan chi tiết (Test/Docs/DoD) → Codex review plan → code + test 3 DB (SQLite, PostgreSQL, MySQL) + Redis
→ docs (guide + core, ADR, CHANGELOG, báo cáo) → Codex verify → sửa gộp, regression test + reverse check
→ full gate → commit (không push).
