# Review cấu trúc thư mục và kiến trúc code

> Ngày: 15/10/2026 · trên `8917a33` (M10) · chỉ review, chưa sửa code.
> Tham chiếu: Laravel (`laravel/framework`, các package `Illuminate\*`) và
> Filament (`filament/filament`, `forms`, `tables`, `support`).

## 1. Kiến trúc hiện tại

### 1.1 Tầng crate (khung lớn: đúng hướng)

```
rustcms            ← crate "umbrella", app chỉ phụ thuộc vào crate này (≈ laravel/framework)
 ├─ rustcms-core   ← nền: app, config, http, db, console, storage, error
 ├─ rustcms-auth   ← guard, session, token, RBAC        (phụ thuộc core)
 ├─ rustcms-admin  ← engine admin kiểu Filament + SPA   (phụ thuộc core, auth)
 └─ rustcms-migration ← migration của framework
rustcms-testing    ← TestApp, TestDb, db_test!, factory
rustcms-cli        ← `rustcms new`, `make:*`            (≈ laravel/installer + artisan make)
admin-ui/          ← React SPA, build vào crates/rustcms-admin/assets
examples/demo      ← app mẫu
```

Phụ thuộc đi một chiều (core ← auth ← admin) và không có vòng: đây là phần làm
tốt, tương đương cách Laravel tách `Illuminate\Foundation`, `Auth`, `Database`.

| Laravel / Filament | rustcms hiện tại |
|---|---|
| `Illuminate\Foundation\Application`, ServiceProvider | `core::app` (`AppBuilder`, `ServiceProvider`, `Inject`) |
| `Illuminate\Http`, middleware | `core::http` |
| `Illuminate\Database`, migrations | `core::db`, `rustcms-migration` |
| `Illuminate\Console` (artisan) | `core::console`, `AppCommand` |
| `Illuminate\Filesystem` | `core::storage` |
| `Illuminate\Auth`, Gate, Policy | `rustcms-auth` (guard, `Gate`, RBAC) |
| Filament Panel | `admin::AdminProvider` |
| Filament Resource | `admin::Resource` |
| Filament Forms / Tables | `admin::ui` (`TextInput::make`, `Table`…) |
| Filament Actions | `admin::action` |
| Livewire/Blade render | `admin::schema` (JSON) + `admin-ui` render |

### 1.2 Design pattern đang dùng

| Pattern | Ở đâu | Ghi chú |
|---|---|---|
| **Service Provider + Container** | `AppBuilder::provider`, `bind`, `Inject` | Giống Laravel. |
| **Facade / re-export** | crate `rustcms`, `prelude` | App chỉ `use rustcms::…`. |
| **Resource (khai báo)** | trait `Resource` + `fields()/form()/table()/policy()/scope()` | Giống Filament Resource. |
| **Type erasure + Adapter** | `ResourceAdapter<R>` implement trait object `DynResource` | Cho phép registry chứa resource khác kiểu entity. Đây là lõi của engine. |
| **Registry** | `Registry` (slug → `Arc<dyn DynResource>`), kiểm cấu hình lúc boot | |
| **Builder** | `TextInput::make().required()`, `Form::new().schema(…)`, `Field::column(…)` | Giống API của Filament. |
| **Schema-driven UI (Interpreter)** | server sinh `ResourceSchema` JSON, SPA chỉ là bộ render | Giống "server-driven UI"; thay cho Livewire. |
| **Policy / Strategy** | trait `Policy`, `Storage`, `KeyStrategy` | Laravel Policy; Strategy cho storage local/S3. |
| **Guard + Extractor** | `Auth`, `AdminCtx` (axum extractor), guard admin/api | Laravel guards. |
| **Active Record** | SeaORM entity (`Model`, `ActiveModel`) | Tương đương Eloquent, không có tầng Repository riêng. |
| **Template Method / hook** | `mutate_before_create`, `after_force_delete`… | Lifecycle hooks của Filament. |
| **Unit of Work** | mọi write trong một transaction, hook nhận `txn` | |
| **Command** | `AppCommand`, `Console` | Artisan. |
| **Receipt / Intent** (độ tin cậy) | idempotency receipts, media upload intents/deletions | Kiểu outbox: ghi ý định trong transaction rồi dọn lại. |

Kết luận phần này: **kiến trúc và pattern hợp lý**. Vấn đề nằm ở việc **sắp xếp file
bên trong từng crate**, đúng như anh thấy.

## 2. Vấn đề cụ thể

### 2.1 `rustcms-admin/src`: 18 file ngang hàng, trộn 4 vai trò

```
action.rs  adapter.rs  context.rs  field.rs  http.rs  idempotency.rs  media.rs
policy.rs  provider.rs  registry.rs  resource.rs  rich_text.rs  schema.rs
spa.rs  system.rs  ui.rs  wire.rs  lib.rs
```

Nhìn tên file không biết file nào là thứ dev app dùng, file nào là phần nội bộ:

| Vai trò | File | Ai dùng |
|---|---|---|
| API khai báo (dev app dùng) | `resource`, `field`, `ui`, `action`, `policy` | dev app |
| Engine nội bộ | `adapter`, `registry`, `context`, `wire`, `idempotency` | framework |
| HTTP / giao tiếp SPA | `http`, `spa`, `schema`, `provider` | framework |
| Tính năng dựng sẵn | `media`, `rich_text`, `system` | cả hai |

### 2.2 File "ôm" quá nhiều việc
- **`adapter.rs`: 1.984 dòng.** Trong một file có cả:
  - nhận input và validate (key/value, rich text);
  - kiểm tham chiếu (belongs_to, media);
  - query: filter, search, sort, phân trang, option và label;
  - lock và transaction;
  - render schema;
  - CRUD, soft delete, action, relation.
- **`rbac.rs`: 1.290 dòng.** Service, các rule chống leo quyền, query và cache quyền
  nằm chung.
- **`media.rs`: 677 dòng.** Entity, resource, policy, handler upload/download, nhận
  diện file và prune nằm chung.
- **`system.rs`: 741 dòng.** DTO, route và handler cho cả users lẫn roles.
- **`ui.rs`: 578 dòng.** Input, layout, form, column, filter, table nằm chung
  (Filament tách thành hai package `forms` và `tables`).
- **`core/storage.rs`: 478 dòng.** Trait, config, provider, driver local và S3 chung
  một file.
- **SPA `pages/system.tsx`: 895 dòng.** Hai trang users và roles chung một file.

### 2.3 Tên gây nhầm
- `ui.rs` là builder (code Rust) còn `schema.rs` là DTO gửi cho SPA. Hai tên này
  không nói lên sự khác nhau đó.
- `context.rs` (`Ctx`), `wire.rs`, `spa.rs`: chỉ hiểu được khi đã đọc code.
- `core::models` (chỉ có `user`) nhưng `auth::entities`: cùng một khái niệm mà hai
  tên.
- `http.rs` của admin chứa cả router lẫn mọi handler.

### 2.4 Không có vấn đề
- Phân tầng crate, hướng phụ thuộc, API public (`rustcms::admin::*`, `prelude`).
- Cấu trúc app sinh ra (`src/admin`, `src/models`, `src/policies`, `src/migrations`,
  `config/`) đã giống Laravel.
- `admin-ui/src` (`api/`, `app/`, `components/`, `lib/`, `pages/`, `renderers/`,
  `schema/`) nhìn chung rõ, chỉ cần tách vài file lớn.
- `tests/` để phẳng là cách làm thông thường của Rust (mỗi file là một binary test).

## 3. Đề xuất cấu trúc

**Nguyên tắc:**
- Nhóm theo vai trò (giống Laravel/Filament).
- Mỗi file một việc, mục tiêu dưới khoảng 500 dòng.
- API public giữ nguyên nhờ `pub use` trong `lib.rs`, nên code app và test không phải
  sửa.
- Chỉ di chuyển code, không đổi hành vi; dùng `git mv` để giữ lịch sử.

### 3.1 `rustcms-admin`

```
src/
├── lib.rs                  # chỉ re-export: API public không đổi
├── panel/                  # ≈ Filament Panel
│   ├── provider.rs         # AdminProvider
│   └── spa.rs              # shell index.html, assets, extensions
├── resources/              # ≈ Filament Resources — API dev app dùng
│   ├── resource.rs         # trait Resource, ResourceMeta, Relation
│   ├── field.rs            # Field, rule, quyền đọc/ghi
│   ├── action.rs           # Action, ActionOutcome
│   ├── policy.rs           # Policy, PermissionPolicy
│   └── query.rs            # ListQuery, Filter, Page, Envelope, Trashed
├── forms/                  # ≈ filament/forms   (tách từ ui.rs)
│   ├── inputs.rs           # TextInput, SelectInput, FileUpload, CustomInput…
│   ├── layouts.rs          # Section, Grid, Tabs, Tab
│   └── form.rs             # Form, Component
├── tables/                 # ≈ filament/tables  (tách từ ui.rs)
│   ├── columns.rs
│   ├── filters.rs
│   └── table.rs
├── engine/                 # nội bộ: CRUD generic, dev app không dùng trực tiếp
│   ├── registry.rs         # Registry, DynResource
│   ├── context.rs          # Ctx
│   ├── wire.rs             # wire format (ADR 0003)
│   ├── idempotency.rs
│   └── adapter/            # tách adapter.rs 1.984 dòng
│       ├── mod.rs          # ResourceAdapter::new, kiểm cấu hình lúc boot
│       ├── input.rs        # accept_input, validate key/value, rich text
│       ├── references.rs   # check_written: belongs_to, media
│       ├── read.rs         # list, find, filter, search_match, options, labels
│       ├── write.rs        # create/update/delete/restore/force, lock, transaction
│       ├── actions.rs      # run_action, disclosed
│       ├── relations.rs    # relation_schemas, bound create
│       └── schema.rs       # render ResourceSchema / form / table
├── http/                   # ≈ Laravel Http: route + controller
│   ├── mod.rs              # router(), danh sách route (cho oracle)
│   ├── extract.rs          # AdminCtx
│   ├── resources.rs        # handler CRUD, schema, options
│   ├── relations.rs
│   └── actions.rs
├── contract/               # DTO gửi SPA (đổi tên từ schema.rs cho rõ nghĩa)
│   └── mod.rs              # ResourceSchema, InputSchema… (ts-rs)
└── features/               # tính năng dựng sẵn, mỗi cái một thư mục
    ├── media/              # entity.rs, resource.rs, upload.rs, download.rs, detect.rs, prune.rs
    ├── rich_text/          # document.rs (grammar), html.rs (to_html)
    └── system/             # users.rs, roles.rs, dto.rs
```

### 3.2 `rustcms-auth`

```
src/
├── lib.rs
├── provider.rs  config.rs  state.rs
├── guards/      # guard.rs (Auth, MaybeAuth), session.rs, tokens.rs, credentials.rs
├── rbac/        # service.rs (RbacService), rules.rs (chống leo quyền), resolver.rs (cache), queries.rs
├── http/        # routes.rs (login/logout/csrf/token)
├── console/     # commands.rs (user:create, role:assign…)
├── entities/    # giữ nguyên
└── support/     # password.rs, secret.rs, rate_limit.rs
```

### 3.3 `rustcms-core`

```
src/
├── lib.rs
├── foundation/  # app.rs (AppBuilder, ServiceProvider), config.rs, key.rs, telemetry.rs
├── http/        # mod.rs (router, middleware), ip.rs, json.rs
├── database/    # connect.rs, check.rs (MySQL), locks.rs (xact_lock), transactions.rs, migrate.rs
├── console/
├── storage/     # mod.rs (trait, key, config, provider), local.rs, s3.rs
├── entities/    # user.rs (đổi từ models/, thống nhất với auth::entities)
├── error.rs
└── principal.rs
```

### 3.4 `admin-ui`
- `pages/system.tsx` → `pages/system/users.tsx`, `pages/system/roles.tsx`, `pages/system/shared.tsx`.
- `pages/list.tsx` (415 dòng), `renderers/form.tsx` (470 dòng): tách component con khi
  cần sửa tiếp, không bắt buộc ngay.

### 3.5 App sinh ra (`rustcms new`)

Đã giống Laravel. Có thể bổ sung dần:

| Laravel | rustcms (đề xuất) |
|---|---|
| `app/Filament/Resources/PostResource.php` | `src/admin/post_resource.rs` (đã có) |
| `app/Models` | `src/models` (đã có) |
| `app/Policies` | `src/policies` (đã có) |
| `app/Http/Controllers`, `routes/api.php` | **thêm** `src/http/` (demo đang có `api.rs` lẻ) |
| `database/migrations`, `database/seeders` | `src/migrations` (đã có), **thêm** `src/seeders/` |
| `resources/js` (Filament asset) | `assets/admin/<vendor>/` (đã có từ M9) |

## 4. Cách làm (nếu anh đồng ý)

- **Một milestone refactor riêng (R1), làm trước M11.** Như vậy M11 (JWT, Redis) sẽ
  thêm code vào cấu trúc mới, không làm file to thêm.
- **Thứ tự làm:**
  1. `rustcms-admin`: phần lợi nhiều nhất, có `adapter.rs`.
  2. `rustcms-auth`.
  3. `rustcms-core`.
  4. SPA `system.tsx`.
- Mỗi bước là một commit riêng, chỉ di chuyển code.
- **Đảm bảo không đổi hành vi:**
  - toàn bộ test trên 3 DB, contract/types không đổi, E2E;
  - `cargo doc` không có link hỏng;
  - API public giữ nguyên (`pub use`), nên `examples/demo` và template không phải sửa.
- **Docs:** thêm trang `docs/src/architecture.md` (sơ đồ crate, bảng đối chiếu
  Laravel/Filament, pattern); cập nhật `CLAUDE.md` và ADR 0002.
- **Rủi ro:**
  - diff lớn nhưng chỉ di chuyển code, dễ review theo từng commit;
  - phải sửa đường dẫn trong docs và các comment có ghi tên file.
