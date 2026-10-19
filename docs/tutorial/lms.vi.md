# Hướng dẫn: dựng một hệ thống LMS với RustCMS

Hướng dẫn này đi từ một app trống tới back office của một trường học trực
tuyến: khoá học, bài học, học viên, ghi danh, tin tức, quyền theo vai trò,
import/export, mail, mời người dùng và dashboard. Mã nguồn hoàn chỉnh nằm ở
`examples/lms`; mỗi bước chỉ ra file tương ứng để đối chiếu.

Lệnh tạo mã (`rustcms new`, `rustcms make:*`) là của CLI `rustcms`; lệnh của
app (`migrate`, `db:seed`, `serve`, `permission:sync`…) chạy bằng `cargo run --`.

Yêu cầu: Rust (rustup), Node 22.12+ chỉ khi sửa giao diện admin (Node 20.19+ đủ để build trang React của app), Docker nếu muốn
dùng PostgreSQL/MySQL/Mailpit. Hướng dẫn dùng SQLite.

## 1. Tạo app

```sh
cargo install --path crates/rustcms-cli   # lệnh `rustcms`
rustcms new school && cd school
cp .env.example .env && cargo run -- key:generate
cargo run -- migrate && cargo run -- serve
```

Cấu trúc thư mục theo Laravel (`src/models`, `src/admin/resources`,
`src/policies`, `src/database/{migrations,seeders,factories}`, `tests/feature`,
`resources/` cho template và script). Xem
[Directory structure](../guide/src/getting-started/directory-structure.md).

Tạo tài khoản quản trị: `cargo run -- make:user --email you@example.test --name "You" --super-admin`.

## 2. Khoá học và bài học

```sh
rustcms make:migration create_courses_table
rustcms make:model Course
rustcms make:resource Course
```

- Migration: `src/database/migrations/m20261020_000001_create_courses_table.rs`
  (dùng helper `create_table`, `instant` để chạy lại được trên MySQL).
- Model: `src/models/course.rs`.
- Resource: `src/admin/resources/course_resource.rs` — field, form, table,
  slug duy nhất (`.unique("courses_slug_key", …)` trả 422 khi trùng).

Bài học (`lesson_resource.rs`) thuộc một khoá học: `belongs_to("courses")`
và `lock_parent("course_id", "courses")` để mọi thao tác ghi khoá khoá học
trước. Action **Lên/Xuống** (`moving`, `reorder`) đổi chỗ bài học với bài kề
bên trong cùng khoá, đánh số lại 1…n và ghi audit từng bài thay đổi.

## 3. Học viên, ghi danh và luật sĩ số

- `learner_resource.rs`, `enrollment_resource.rs`.
- Luật nghiệp vụ nằm trong service `src/services/enrollment_service.rs`
  (`enroll_in` khoá khoá học rồi đếm chỗ trống) — dùng chung cho panel,
  action "Ghi danh…" có form (`Action::form`) và seeder.

## 4. Quyền theo vai trò

- Mỗi resource có quyền `<slug>.view_any/view/create/update/delete`; quyền
  riêng khai báo trong `src/policies/permissions.rs`.
- Giảng viên chỉ thấy khoá mình dạy: `scope()` của resource lọc theo
  `course::taught_by` — áp dụng cho list, chi tiết, quan hệ, action, export.
- Vai trò mẫu (`src/database/seeders/role_seeder.rs`): `staff`, `instructor`,
  `editor` (chỉ tin tức), `viewer` (chỉ xem, export ghi danh).
- Sau khi thêm quyền: `cargo run -- permission:sync`.

## 5. Tin tức: rich text, ảnh bìa, tag, slug

`news_resource.rs`:

- `Field::column(news::Column::Body).rich_text()` và
  `Field::column(news::Column::Cover).media().accept(&["image/*"])` (cần
  `.media()` trên `AdminProvider` và `StorageProvider`).
- Tag là quan hệ nhiều-nhiều qua bảng `news_tags`:
  `Relation::belongs_to_many("tags", "tags", news_tag::Column::NewsId, news_tag::Column::TagId)`;
  gắn/gỡ tag cần quyền `news.attach_tags` / `news.detach_tags`.
- Slug tự sinh từ tiêu đề trong hook `mutate_before_create` (`slugify` bỏ dấu
  tiếng Việt).
- "Hẹn giờ đăng": `published_at` ở tương lai.

## 6. Import và export CSV

- Học viên import được theo email: `ResourceMeta::new("Learner").import_key("email")`.
- Ghi danh export được: `ResourceMeta::new("Enrollment").exportable()`; quyền
  `enrollments.export`. Xem [Import and export](../guide/src/admin/import-export.md).

## 7. Mail và job

- Cấu hình `config/mail.toml` (driver `log` khi phát triển; `smtp` khi chạy
  thật, server nhập ở **Settings → Mail**, giới hạn bằng `allowed_hosts`).
- Job `src/jobs/send_welcome.rs` gửi mail chào mừng cho từng cặp khoá
  học/học viên rồi ghi receipt (template `resources/views/mail/{vi,en}/welcome.*`);
  nếu bị ngắt sau khi gửi, lần chạy lại có thể gửi trùng. Ghi danh dispatch job
  trong cùng transaction.
- Dev: `docker compose up mailpit`, xem thư ở <http://localhost:58025>.

## 8. Quên mật khẩu, mời người dùng, ghi nhớ đăng nhập

- `config/auth.toml`: `password_reset = true` (cần mail).
- **System → Users → New user → "Gửi lời mời thay vì đặt mật khẩu"**: giảng viên
  nhận thư và tự chọn mật khẩu.
- Ô "Ghi nhớ đăng nhập" giữ phiên `remember_days` ngày.

## 9. Dashboard

`src/providers/admin_panel_provider.rs`:

```rust
Dashboard::new()
    .widget(Widget::stat("learners", "learners").label("Learners"))
    .widget(Widget::stat("new_enrollments", "enrollments").compare_last_days(30, "enrolled_at"))
    .widget(Widget::chart("enrollments_per_week", "enrollments", "enrolled_at").per(Bucket::Week).last(12))
    .widget(Widget::recent("new_courses", "courses").columns(&["capacity"]).sort("-created_at"))
```

Mỗi widget có quyền `dashboard.<tên>` và chỉ đếm những gì người dùng được xem.

## 10. Tiếng Việt

Nhãn dịch trong `lang/vi.toml`; `cargo run -- lang:missing vi` liệt kê nhãn
còn thiếu. Người dùng chọn ngôn ngữ ở trang Account.

## 11. Kiểm thử

`tests/feature/*` chạy trên SQLite, PostgreSQL và MySQL nhờ `db_test!`:

```sh
cargo nextest run                              # SQLite
RUSTCMS_TEST_PG=… RUSTCMS_TEST_MYSQL=… cargo nextest run
```

## 12. Chạy thật

- Lưu file trên S3/R2: `config/storage.toml` (`disk = "s3"`, feature `s3`).
- Nhiều instance: Redis (feature `redis`).
- Vận hành: [Operations](../guide/src/operations/queue.md) — hàng đợi, thời gian
  lưu giữ, sao lưu/khôi phục, xoay `APP_KEY`, mail.
