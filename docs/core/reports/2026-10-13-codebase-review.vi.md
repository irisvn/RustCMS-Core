# Báo cáo review codebase rustcms 0.1

> Ngày: 13/10/2026 · nhánh `main` (commit `dc6e5ac`) · phạm vi: toàn bộ workspace
> Rust, admin SPA, CLI/template, docs, CI.

## 1. Kết luận

- **Yêu cầu ban đầu**: đạt phần lớn. Backend, Admin và API chung một source;
  Resource khai báo bằng Rust sinh API CRUD có phân quyền và SPA nhúng; hai guard
  tách biệt (session + CSRF cho admin, token cho API); RBAC qua `RbacService`;
  PostgreSQL + SQLite. Các mục hoãn sang 0.2 (JWT, MySQL, upload, RichEditor,
  soft delete, Redis) được ghi rõ.
- **Bảo mật**: nền tảng tốt (deny-by-default, whitelist field, scope/policy phía
  server, CSRF + Origin, secret được hash, khóa dòng khi ghi, CSP chặt). Có
  **2 lỗi P1** và **4 lỗi P2** cần sửa trước khi phát hành 0.1.
- **Maintain/mở rộng**: kiến trúc rõ (Resource → adapter type-erased → registry;
  contract fixtures; authz oracle; checklist review). Rủi ro chính là
  `adapter.rs` lớn, một số lời hứa trong docs chưa có code tương ứng, và MSRV
  khai báo sai.

## 2. Đối chiếu yêu cầu

| Yêu cầu | Trạng thái | Ghi chú |
|---|---|---|
| Backend + Admin + API chung source | Đạt | Một binary, SPA nhúng |
| Resource → CRUD + UI tự sinh | Đạt | Schema-driven, contract fixtures |
| Guard admin (session) / API (token) | Đạt | Tách middleware, CSRF đồng bộ |
| RBAC role/permission | Đạt | Reload grants dưới khóa |
| PostgreSQL + SQLite | Đạt | `db_test!` chạy cả hai |
| Đổi `APP_KEY` thu hồi session | **Chưa đúng** | Docs/CLI hứa nhưng key không được dùng (S2) |
| MSRV 1.90 | **Sai** | SeaORM 2.0.4 cần 1.94 (R1) |

## 3. Findings bảo mật / toàn vẹn dữ liệu

| ID | Mức | Vấn đề | Vị trí |
|---|---|---|---|
| S1 | P1 | Sau khi ghi, `committed_envelope` hydrate bằng `envelopes` không kiểm lại `scope()`: tạo/sửa bản ghi ra ngoài scope (vd. category tên "Hidden…") vẫn trả `data`/title/labels | `crates/rustcms-admin/src/adapter.rs` (`committed_envelope`) |
| S2 | P1 | `key:generate --force` chỉ ghi lại `.env`; session id là SHA-256 không khóa nên session cũ vẫn sống, trái với thông báo "existing sessions become invalid" | `crates/rustcms-core/src/console.rs:227`, `key.rs:1`, `crates/rustcms-auth/src/session.rs:149` |
| S3 | P2 | Generator so tên cột nhạy cảm (`password`, `token`…) và cột owner chỉ theo tên DB; alias `#[sea_orm(column_name)]` lọt qua → cột bí mật thành field đọc/ghi | `crates/rustcms-cli/src/resource.rs:141–151` |
| S4 | P2 | Existence oracle: `find`/`schema`/ghi tra bản ghi trước khi kiểm quyền → 404 (không tồn tại) vs 403 (tồn tại) | `adapter.rs` `schema()`, `find`, `update`, `delete`, `run_action` |
| S5 | P2 | `expires_in_days` (u32) không giới hạn → `Utc::now() + Duration::days(..)` panic với giá trị lớn (HTTP và CLI) | `crates/rustcms-auth/src/routes.rs:275`, `commands.rs:111` |
| S6 | P2 | Decode số thực tạo ứng viên `f32` từ `f64` không kiểm range: `1e100` → `Infinity` được ghi vào cột Float rồi đọc ra `null` | `crates/rustcms-admin/src/wire.rs:143–146` |

## 4. Correctness / release / lifecycle

| ID | Vấn đề | Vị trí |
|---|---|---|
| R1 | `rust-version = "1.90"` nhưng `sea-orm 2.0.4` yêu cầu 1.94; CI chỉ dùng stable | `Cargo.toml:8`, `.github/workflows/ci.yml` |
| R2 | Generator: khóa String tự nhiên bị `continue` → không có field/form để nhập khóa, create luôn lỗi | `crates/rustcms-cli/src/resource.rs:126–137` |
| R3 | `new` chấp nhận tên package là keyword Rust (`async`) → template không biên dịch | `crates/rustcms-cli/src/naming.rs:78–85` |
| R4 | `make:user` không giới hạn độ dài mật khẩu tối đa (HTTP có) | `crates/rustcms-auth/src/commands.rs:43–66` |
| R5 | Renewal session gửi lại cookie; response renewal đến trễ có thể ghi đè cookie vừa rotate → bị đăng xuất | `crates/rustcms-auth/src/session.rs:239–283` |
| R6 | Test `assignment_needs_its_own_permission` bỏ quyền trên `Principal` nhưng `RbacService` reload từ DB → không kiểm đúng gate | `crates/rustcms-auth/tests/rbac.rs:28–52` |
| R7 | Docs revocation chưa chính xác; `just ci` không gồm security/test-cli/e2e nhưng mô tả là "everything CI runs"; `http_stack` chỉ SQLite; thiếu regression `migrate:fresh` + FK | `docs/src/api.md:43–45`, `justfile:7`, `crates/rustcms-testing/tests/http_stack.rs` |

## 5. Admin UI

| ID | Vấn đề | Vị trí |
|---|---|---|
| U1 | Blind edit: khi quyền view xuất hiện, form bị thay bằng `<Loading/>` → mất draft | `admin-ui/src/pages/record.tsx:42` |
| U2 | Ô search không đồng bộ khi URL đổi (back/forward) | `admin-ui/src/pages/list.tsx:66` |
| U3 | Trang Account bỏ `formErrors` | `admin-ui/src/pages/account.tsx:13–16` |
| U4 | Read-only password hiện plain text; select tĩnh có giá trị boolean mất nhãn | `admin-ui/src/renderers/form.tsx:93,100` |
| U5 | Field hidden nhưng required chặn submit, lỗi không hiển thị | `admin-ui/src/schema/form.ts`, `renderers/form.tsx:113` |
| U6 | Kiểm i32 nhận `2147483648` | `admin-ui/src/lib/wire.ts:28` |
| U7 | Account/System không hiển thị trạng thái tạm dừng do ghi chưa rõ kết quả; `system.tsx:78` copy chuỗi CSS của `buttonVariants` | `admin-ui/src/pages/*.tsx` |

## 6. Maintainability

- Tốt: ranh giới crate rõ, ADR, contract fixtures server↔SPA, authz oracle
  (`fixtures/authz/*.toml`), `db_test!`, `docs/review-checklist.md`.
- Cần cải thiện: `adapter.rs` gom nhiều trách nhiệm (nên tách read/write path ở
  0.2); policy tùy biến chưa có cách khai báo "không phụ thuộc bản ghi" cho mọi
  ability; template app và `examples/demo` đã lệch nhau.

## 7. Điểm mạnh cần giữ

Tách guard; CSRF đồng bộ + kiểm Origin; secret/token được hash; RBAC reload dưới
khóa; khóa dòng và kiểm lại policy khi ghi; whitelist field; CSP
`script-src 'self'; style-src 'self'`. Ghi đang chạy vẫn commit khi quyền bị thu
hồi giữa chừng và cache/rate limit theo tiến trình (single-instance) là **ranh
giới có chủ đích của 0.1**, không phải lỗi.

## 8. Giới hạn của lần review

Đã chạy: lint/typecheck/Vitest (14 file, 107 test), `npm audit` (0), quét
RustSec cục bộ (0 match; không phải `cargo-audit` chính thức), probe biên dịch
cho S6 và probe render cho U4/U6. Chưa chạy trong lần review: matrix Rust hai
DB, scaffold/package, MSRV, E2E.

## 9. Thứ tự sửa

1. Batch 1 (S1–S6), mỗi lỗi có regression test chứng minh fail-trước/pass-sau.
2. Batch 2 (R1–R7, U1–U7), cập nhật docs/CHANGELOG.
3. Tự rà theo `docs/review-checklist.md`, một vòng review song song + một vòng
   Codex verify; sau đó mới lập plan 0.2.

Backlog 0.2 (không chặn 0.1): giới hạn số token phát hành, ngữ nghĩa limiter
`max = 0`, chuẩn hóa origin cổng mặc định, nhận diện `https://` không phân biệt
hoa thường, cập nhật `last_used_at` của token kiểu best-effort.

## 10. Trạng thái xử lý (13/10/2026, chưa commit)

Mỗi fix có regression test, đã kiểm chứng **fail trước fix / pass sau fix**
(reverse check).

| ID | Trạng thái | Cách sửa · test |
|---|---|---|
| S1 | Đã sửa | `committed_envelope` đọc lại trong `scope()`; ngoài scope → chỉ key · `write_responses_respect_the_scope_after_the_write` |
| S2 | Đã sửa | Session id = HMAC-SHA256(APP_KEY, token); đổi key ⇒ mọi session cũ 401 (API token giữ nguyên) · `rotating_the_app_key_signs_everyone_out` |
| S3 | Đã sửa | Kiểm cả tên field Rust lẫn `column_name` · `renamed_columns_are_checked_under_both_names` |
| S4 | Đã sửa | `Policy::admits` kiểm trước khi tra key (403 đồng nhất); blind edit vẫn chạy · `lookups_by_key_do_not_reveal_existence_without_the_ability` |
| S5 | Đã sửa | Giới hạn 1–3.650 ngày, 422 / lỗi CLI · `token_lifetimes_are_bounded` |
| S6 | Đã sửa | Không tạo ứng viên `f32` vô hạn; 422 "is out of range" · `floats_out_of_f32_range_are_rejected` |
| R1 | Đã sửa | MSRV 1.94, job CI `msrv`, `just msrv` (đã chạy: `cargo +1.94 check --workspace --all-targets` sạch) |
| R2, R3 | Đã sửa | Khóa String tự nhiên có input bắt buộc; từ khóa Rust bị từ chối · unit test CLI |
| R4 | Đã sửa | `make:user` giới hạn `MAX_PASSWORD_LEN` |
| R5 | Đã sửa | Cookie không `Max-Age`, renewal không gửi lại cookie · `sliding_renewal_does_not_resend_the_cookie` |
| R6 | Đã sửa | Test dùng grants thật trong DB, có reverse case |
| R7 | Đã sửa | Docs revocation/`just ci`; ghi chú `http_stack`; `db::migrate_fresh` + `fresh_drops_tables_with_foreign_keys` |
| U1–U7 | Đã sửa | Vitest mới trong `record-form`, `list`, `account`, `read-only`, `form`, `wire` |

Verification: `cargo nextest` toàn workspace SQLite + PostgreSQL 324/324 pass;
clippy `-D warnings`, rustfmt, rustdoc; Vitest 113 test, ESLint, tsc; contract
fixtures và TS types không đổi; `docs/src/cli.md` và assets SPA đã tạo lại;
E2E Playwright 27/27; `just test-cli` (app sinh ra build và pass test);
`just msrv`. Chưa chạy local: cargo-deny (CI chạy; dependency mới duy nhất là
`hmac 0.13`, vốn đã có sẵn trong `Cargo.lock`).

Còn lại (backlog 0.2): tra cứu option checkbox-list bằng Map (hiệu năng), cùng
các mục hardening ở mục 9.
