# rustcms — Tài liệu tổng quan (bản 0.1)

> Cập nhật: 13/10/2026 · nhánh `main` (commit `ce2db1e`)

rustcms là framework Rust dùng để dựng nhanh **CMS và Admin Dashboard**, lấy cảm
hứng từ Laravel và FilamentPHP:

- Backend, Admin và API dùng **chung một source**.
- Khai báo *Resource* bằng Rust, framework tự sinh API CRUD có phân quyền và
  **giao diện admin** (React, nhúng sẵn trong binary, người dùng framework không
  cần cài Node).
- Phân quyền có sẵn (role/permission), với **2 guard tách biệt**: Admin (session)
  và API (token).

---

## 1. Khởi động nhanh

### Yêu cầu
- **Rust** stable (≥ 1.94). Trên máy anh: `export PATH=/opt/homebrew/opt/rustup/bin:$PATH`.
- Tuỳ chọn: **Docker** để test với PostgreSQL; **Node 20+** chỉ khi sửa mã nguồn admin SPA.

### Chạy app demo (blog có sẵn dữ liệu)

```sh
cd examples/demo
cp .env.example .env && cargo run -- key:generate   # chỉ cần lần đầu (máy anh đã có .env)
cargo run -- migrate          # tạo/cập nhật bảng (SQLite: examples/demo/database.sqlite)
cargo run -- demo:seed        # tạo tài khoản + dữ liệu mẫu (chạy lại an toàn)
cargo run -- serve            # chạy server
```

Mở **http://localhost:8000/admin**.

> Dùng đúng `localhost:8000` (khớp `APP_URL` trong `.env`). Mở bằng `127.0.0.1`
> thì trang vẫn hiện nhưng đăng nhập bị chặn (403), vì đăng nhập và mọi thao tác
> ghi chỉ chấp nhận origin trùng `APP_URL`.

### Tài khoản

| Email | Mật khẩu | Vai trò |
|---|---|---|
| **admin@example.test** | `password123` | **Super admin** (guard admin + api): thấy và làm được mọi thứ, gồm mục *System → Users/Roles* |
| editor@example.test | `password123` | Editor: xem categories; tạo bài; chỉ xem/sửa **bài của chính mình** |

Muốn làm sạch dữ liệu demo: `cargo run -- migrate:fresh && cargo run -- demo:seed`.

### Tạo app mới và super admin của riêng anh

```sh
cargo install --path crates/rustcms-cli         # cài lệnh `rustcms`
rustcms new myapp && cd myapp
rustcms migrate
rustcms make:user --email anh@example.com --name "Anh" --super-admin    # super admin của guard admin
rustcms role:assign --email anh@example.com --role super_admin --guard api   # (tuỳ chọn) quyền API
rustcms serve                                    # http://localhost:8000/admin
```

`make:user` in ra mật khẩu ngẫu nhiên nếu không truyền `--password`.

### Thử API bằng token (demo)

```sh
cargo run -- token:create --email admin@example.test --name cli --abilities posts.view_any,posts.create
curl -H "Authorization: Bearer rcms_…" http://localhost:8000/api/v1/posts
curl -H "Authorization: Bearer rcms_…" -H "Content-Type: application/json" \
     -d '{"title":"Từ API"}' http://localhost:8000/api/v1/posts
```

### Các service khi phát triển framework

```sh
docker compose up -d --wait   # PostgreSQL ở localhost:55439 (cổng 55432 là của dự án khác)
just test-matrix              # test Rust trên SQLite + PostgreSQL
just test-ui                  # test SPA (Vitest)
just e2e                      # test trình duyệt (Playwright) — máy anh: PLAYWRIGHT_CHANNEL=chrome
just dev                      # Rust :8000 + Vite :5173 (mở http://localhost:5173/admin/)
just ci                       # phần lớn CI; thêm `just test-cli`, `just e2e`, `just msrv`
just publish-check            # đóng gói + verify các crate cho crates.io
```

---

## 2. Kiến trúc

```
crates/
├── rustcms            facade + prelude (app chỉ cần phụ thuộc crate này)
├── rustcms-core       AppBuilder, ServiceProvider, config, lỗi, HTTP stack, Console (`artisan`)
├── rustcms-auth       guard session/token, CSRF, rate limit, RBAC (RbacService), lệnh console
├── rustcms-admin      engine Resource → JSON schema → CRUD; API admin; trang System; phục vụ SPA
├── rustcms-migration  bảng users, sessions, tokens, roles/permissions
├── rustcms-testing    TestApp, acting_as / login_as / with_token, factory, DB riêng mỗi test
└── rustcms-cli        `rustcms new`, make:*, chuyển tiếp lệnh runtime
admin-ui/              React 19 + Vite + Tailwind + TanStack (build vào crates/rustcms-admin/assets)
examples/demo/         app blog mẫu
docs/guide/            mdBook public (tiếng Anh)
docs/core/             tài liệu nội bộ: kiến trúc, ADR, báo cáo, CHANGELOG
fixtures/              hợp đồng JSON server↔SPA, oracle phân quyền viết tay
```

**Luồng chính:** `Resource` (Rust) → `ResourceSchema` JSON (đã lọc theo quyền của
từng user) → SPA render form/bảng/bộ lọc → gọi API CRUD → engine kiểm quyền ở mức
field và record, rồi ghi qua SeaORM trong transaction.

---

## 3. Tính năng theo milestone

| Milestone | Nội dung chính |
|---|---|
| **M0** | Workspace, CI, spike engine CRUD tổng quát, wire format (i64/decimal dạng chuỗi) |
| **M1** | Core: config + APP_KEY, AppBuilder/provider, lỗi kiểu Laravel, `/health`, Console, migration, `rustcms new`, `make:model/migration`, bộ test kit |
| **M2** | Auth: session + CSRF + kiểm Origin, token kiểu Sanctum, argon2, rate limit, thu hồi credential tức thì, RBAC chống leo thang quyền (`RbacService`), `make:user`, `token:create`, `permission:sync` |
| **M3** | Engine Resource: builder form/bảng/bộ lọc/action, schema có version, quyền theo field và record (cả relationship và options), giới hạn truy vấn, map lỗi constraint, khoá dòng khi ghi |
| **M4** | **Admin SPA** nhúng trong binary: đăng nhập, menu, danh sách (search/sort/filter/phân trang/bulk action), form create/edit/view đủ vocabulary 0.1, CSP chặt, chống mất bản nháp và ghi trùng |
| **M5** | Trang **System**: Users, Roles (ma trận permission), **Account** (đổi mật khẩu, đăng xuất nơi khác); mọi thao tác qua `RbacService` |
| **M6** | `make:resource --generate`, `make:policy`, `route:list`, `role:assign`, ví dụ API token `/api/v1/posts`, docs đầy đủ, license, sẵn sàng publish |

### Tạo một màn hình admin trong 3 lệnh

```sh
rustcms make:model Post -m        # sửa model + migration (thêm cột)
rustcms make:resource Post --generate --belongs-to category_id=categories
rustcms migrate && rustcms permission:sync
```

`--generate` đọc các cột của model để sinh field, form, bảng và bộ lọc. Cột nhạy
cảm (password, token…) và kiểu chưa hỗ trợ bị bỏ qua kèm cảnh báo. Cột chủ sở hữu
(`author_id`…) do server gán, client không ghi được.

---

## 4. Bảo mật (điểm chính)

- **Deny by default.** Mọi đường (danh sách, options, relationship, action, response
  của thao tác ghi) đều kiểm quyền field và record. Cột chưa khai báo không bao giờ bị lộ.
- **2 guard tách biệt.** Admin dùng session cookie (`HttpOnly`, `SameSite=Lax`, giới
  hạn trong `/admin`) kèm CSRF và kiểm Origin. API dùng token có abilities, được giao
  với quyền thực của user.
- **Chống leo thang quyền:**
  - Chỉ cấp hoặc gỡ được những permission mình đang có; không tự sửa quyền của mình;
    role được bảo vệ chỉ super admin mới sửa.
  - Luôn còn ít nhất một super admin.
  - Đổi mật khẩu hoặc email của người khác chỉ dành cho super admin của **mọi** guard.
- **Thu hồi tức thì.** Đổi mật khẩu, đăng xuất mọi nơi, vô hiệu hoá user hay đổi
  email đều làm mất hiệu lực session và token từ request kế tiếp.
- **Chống race.** Khoá dòng khi ghi; kiểm tra lạc quan (`expected`) để bản nháp cũ
  không ghi đè được; kiểm tra lại `auth_version` dưới lock.
- **Phía trình duyệt:**
  - CSP `script-src 'self'; style-src 'self'`.
  - Phát hiện khi cookie chuyển sang tài khoản khác, hoặc khi đang xoay vòng
    thông tin đăng nhập ở tab khác.
  - Thao tác ghi không rõ kết quả thì khoá gửi lại, tránh ghi trùng.

---

## 5. Chất lượng và quy trình

| Hạng mục | Kết quả cuối |
|---|---|
| Test Rust | **309 pass** trên PostgreSQL và SQLite |
| Test SPA (Vitest) | **85 pass** |
| E2E trình duyệt (Playwright) | **10 pass**, không vi phạm CSP |
| Test scaffold (`rustcms new` + generator → app compile và test pass) | pass |
| Lint | clippy `-D warnings`, eslint, tsc; rustdoc `-D warnings`; mdBook |
| Publish | `cargo publish --workspace --dry-run`: cả 7 crate đóng gói và verify thành công |

**Review:**
- Codex adversarial-review theo từng milestone, kèm stop-gate review.
- Từ M5 chuyển sang **review toàn diện song song**: nhiều reviewer theo từng nhóm
  vấn đề, sửa gộp một đợt rồi xác minh lại. Cách này rút từ 10–22 vòng xuống 2–3 đợt.
- Mỗi lỗi sửa đều có test regression, và reverse check (bỏ bản sửa thì test phải fail).
- Checklist rút ra từ các lỗi đã gặp: `docs/core/review-checklist.md`.

---

## 6. Giới hạn đã biết của 0.1

- Chỉ chạy **một instance**: rate limit và cache quyền nằm trong bộ nhớ.
- Chưa có: MySQL, JWT, quên mật khẩu và xác minh email, upload file, rich text,
  soft delete, sửa quan hệ nhiều-nhiều trên giao diện, action có form modal,
  khoá chính nhiều cột, idempotency key.
- Đăng xuất một session hoặc thu hồi một token không huỷ được thao tác ghi đang
  chạy dở bằng chính credential đó. Đổi mật khẩu và đăng xuất mọi nơi thì có hiệu lực.
- Vòng Codex cuối cho M6 chưa chạy vì server Codex lỗi. M6 được review bởi 2 reviewer
  toàn diện, và 7 điểm còn lại của M5 do Codex nêu đã được sửa trong cùng đợt.
- Kích thước: binary release của demo khoảng 23 MB (18 MB sau `strip`); JS của admin
  khoảng 0.55 MB (175 KB gzip).

---

## 7. Lộ trình

- **0.2 (đã xong, M7–M11 + R1/R2):**
  - JWT guard kèm refresh token xoay vòng (M11), MySQL 8.4 (M10).
  - RichEditor, Upload/Media, Tabs/KeyValue, field tuỳ biến (M7–M9).
  - Soft delete, relationship manager (M7, M9).
  - Chạy nhiều instance với Redis (rate limit, job định kỳ); quyền đồng bộ qua
    database (M11); idempotency key (M7).
  - Tầng API cho ứng dụng (`ApiRoutes`, `make:api`, R2); cấu trúc thư mục theo vai trò (R1).
- **Sau 0.2:** Events, Settings, Queue/Jobs, Mail, quên mật khẩu và xác minh email,
  Audit log, Pages/Menus kèm headless API, i18n, Dashboard widgets, Plugins.

---

## 8. Tài liệu chi tiết

- Bắt đầu: `docs/guide/src/getting-started/installation.md`
- Resource và Admin: `docs/guide/src/admin/resources.md`, `docs/guide/src/admin/overview.md`,
  `docs/guide/src/admin/users-roles.md`
- API token: `docs/guide/src/api/authentication.md`
- Bảo mật: `docs/guide/src/security/*`
- Giao thức admin (server ↔ SPA): `docs/core/admin-protocol.md`
- Lệnh CLI: `docs/guide/src/reference/cli.md`
- Quyết định kiến trúc: `docs/core/adr/`
- Báo cáo review: `docs/core/reports/` (vd. `2026-10-13-codebase-review.vi.md`)
- Xem docs dạng web: `mdbook serve docs` → http://localhost:3000
