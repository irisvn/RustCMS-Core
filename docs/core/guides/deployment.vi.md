# Build và chạy binary (tiếng Việt)

Một ứng dụng rustcms (tạo bằng `rustcms new`, hoặc `examples/demo`) được build
thành **một file binary duy nhất**. Binary này chứa:
- server HTTP;
- toàn bộ lệnh console (`migrate`, `make:user`, …);
- admin panel (SPA đã nhúng sẵn);
- bản dịch giao diện (`lang/*.toml` được nhúng lúc build, không cần copy).

Vì vậy máy chạy **không cần Rust, Node hay mã nguồn**.

Trong trang này, binary tên `blog`. Với demo, tên của nó là `demo`.

## 1. Build bản release

```sh
cargo build --release          # → target/release/blog
strip target/release/blog      # tuỳ chọn: khoảng 29 MB → 23 MB
```

- **Feature** khai báo trong `Cargo.toml` của app, ở dependency `rustcms`:

  | Feature | Mặc định | Dùng khi |
  |---|---|---|
  | `sqlite`, `postgres`, `mysql` | bật | driver database |
  | `redis` | tắt | chạy nhiều instance (mục 7) |
  | `s3` | tắt | lưu upload trên S3 / MinIO (`storage.disk = "s3"`) |

  ```toml
  rustcms = { git = "https://github.com/irisvn/RustCMS-Core", tag = "v0.9.0", features = ["redis", "s3"] }  # giữ tag app đang dùng
  ```
- **Admin SPA** đã build sẵn trong crate `rustcms-admin`. App bình thường không
  cần chạy `npm`. Chỉ khi sửa mã nguồn SPA trong repo rustcms mới phải chạy
  `just admin-build` trước `cargo build`.
- **Build cho Linux** từ macOS: build trên máy Linux (CI) hoặc trong Docker
  (mục 6). Binary của macOS không chạy được trên Linux.

## 2. Binary cần những gì khi chạy

Binary đọc mọi thứ theo **thư mục đang đứng** (working directory):

| Thứ | Ở đâu | Ghi chú |
|---|---|---|
| Cấu hình | `config/*.toml` | tuỳ chọn: thiếu file nào thì dùng mặc định |
| Biến môi trường | `.env`, hoặc environment của process | biến có sẵn trong environment luôn thắng `.env` |
| Upload (disk `local`) | `storage/app` (`storage.root`) | **phải giữ lại** giữa các lần deploy, và backup |
| SQLite | theo `DATABASE_URL` | nên dùng đường dẫn tuyệt đối |

Ví dụ bố cục trên server:

```text
/opt/blog/
├── blog              # binary
├── config/           # copy từ repo (app.toml, auth.toml, http.toml, log.toml…)
├── .env              # bí mật (chmod 600), hoặc dùng EnvironmentFile của systemd
└── storage/app/      # file upload (user chạy app phải ghi được)
```

**Biến môi trường chính:**

| Biến | Bắt buộc | Ví dụ |
|---|---|---|
| `APP_KEY` | có | `base64:…`: tạo một lần bằng `./blog key:generate --show`, lưu như mật khẩu |
| `DATABASE_URL` | có | `postgres://user:pass@db:5432/blog`, `mysql://…`, `sqlite:///opt/blog/data/app.sqlite?mode=rwc` |
| `APP_ENV` | nên đặt | `production` (đây cũng là mặc định) |
| `APP_URL` | có | `https://blog.example.com`: origin được phép đăng nhập và ghi dữ liệu trong admin |
| `APP_NAME`, `APP_DEBUG` | không | `APP_DEBUG=false` khi chạy production |
| `REDIS_URL` | khi nhiều instance | `redis://redis:6379/0` |
| `RUST_LOG` | không | `info`, ghi đè `log.filter` |
| `RUSTCMS_<SECTION>__<KEY>` | không | `RUSTCMS_APP__LISTEN=0.0.0.0:8000`, `RUSTCMS_HTTP__TRUSTED_PROXIES='["10.0.0.0/8"]'` |

Nếu đặt đủ bằng biến môi trường thì có thể bỏ hẳn thư mục `config/`. Mọi khoá
cấu hình có trong [Configuration](../../guide/src/getting-started/configuration.md).

> Đổi `APP_KEY` sẽ đăng xuất mọi session admin và kết thúc mọi phiên JWT
> (API token `rcms_…` vẫn dùng được). Không commit `APP_KEY`.

## 3. Các lệnh thường dùng

Binary chính là `rustcms`/`artisan` của app. Chỉ các mục Application commands, Auth commands và Admin
panel commands trong [CLI](../../guide/src/reference/cli.md) chạy bằng `./blog <lệnh>`; Global commands chạy
bằng lệnh `rustcms` đã cài:

```sh
./blog key:generate --show                # in ra APP_KEY mới (không ghi file)
./blog migrate                            # chạy migration còn thiếu
./blog migrate:status                     # xem migration nào đã chạy
./blog migrate:rollback --step 1          # lùi 1 migration
./blog permission:sync                    # tạo permission mới khai báo trong code
./blog make:user --email you@example.com --name You --super-admin   # in mật khẩu sinh ngẫu nhiên
./blog role:assign --email you@example.com --role super_admin --guard api
./blog token:create --email you@example.com --name deploy --abilities posts.view_any --expires-days 30
./blog route:list                         # mọi route và quyền của nó
./blog session:prune                      # dọn session / phiên JWT hết hạn (serve tự làm mỗi giờ)
./blog serve                              # chạy server (app.listen, mặc định 127.0.0.1:8000)
./blog serve --listen 0.0.0.0:8000
./blog --help                             # liệt kê mọi lệnh, kể cả lệnh riêng của app
```

- Không truyền `--password` cho `make:user` trên server: mật khẩu sẽ lưu vào
  shell history. Để trống thì lệnh tự sinh mật khẩu và in ra một lần.
- `migrate:fresh` và `migrate:refresh` xoá dữ liệu. Ở production, hai lệnh này
  bắt buộc có `--force`.
- `serve` dừng êm khi nhận Ctrl-C hoặc SIGTERM: request đang chạy được làm cho xong.

## 4. Lần chạy đầu tiên

```sh
cd /opt/blog
./blog key:generate --show        # copy giá trị vào .env hoặc secret store: APP_KEY=base64:…
./blog migrate
./blog permission:sync
./blog make:user --email you@example.com --name You --super-admin
./blog serve
```

Kiểm tra:
- `curl http://127.0.0.1:8000/health` phải trả `{"status":"ok","database":"ok"}`.
  Load balancer nên dùng URL này làm health check.
- Mở `APP_URL` + `/admin` và đăng nhập. Phải mở đúng host của `APP_URL`: mở
  bằng origin khác thì vẫn thấy trang, nhưng đăng nhập bị 403.

## 5. Deploy bản mới

1. Build binary mới, copy lên server bên cạnh bản cũ (ví dụ `blog.new`).
2. Backup database (và `storage/app` nếu dùng disk local).
3. `./blog.new migrate`, rồi `./blog.new permission:sync`.
4. Thay binary (`mv blog.new blog`), rồi restart service.
5. Gọi `/health` và đăng nhập thử.

**Quay lại bản cũ:**
- `./blog migrate:rollback --step N` bằng **binary mới**, vì chỉ nó biết các
  migration mới. Sau đó mới chạy lại binary cũ.
- Migration có xoá cột hay bảng thì không lùi lại được dữ liệu: khi đó dùng
  bản backup.

## 6. Ví dụ chạy dịch vụ

> Các file dưới đây là mẫu, cần chỉnh đường dẫn và tên cho đúng app của anh.

### systemd

`/etc/systemd/system/blog.service`:

```ini
[Unit]
Description=blog (rustcms)
After=network-online.target

[Service]
User=blog
WorkingDirectory=/opt/blog
EnvironmentFile=/opt/blog/.env
ExecStart=/opt/blog/blog serve
Restart=on-failure
# Chỉ cho ghi vào chỗ cần ghi.
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/opt/blog/storage /opt/blog/data

[Install]
WantedBy=multi-user.target
```

```sh
sudo systemctl daemon-reload && sudo systemctl enable --now blog
journalctl -u blog -f            # xem log (đặt log.json = true nếu gửi log đi nơi khác)
```

### Docker (nhiều stage)

```dockerfile
FROM rust:1.94-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release && strip target/release/blog

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* && useradd --system --home /app blog
WORKDIR /app
COPY --from=build /src/target/release/blog /app/blog
COPY config /app/config
RUN mkdir -p /app/storage/app && chown -R blog /app/storage
USER blog
ENV APP_ENV=production RUSTCMS_APP__LISTEN=0.0.0.0:8000
EXPOSE 8000
CMD ["/app/blog", "serve"]
```

```sh
docker build -t blog .
docker run --rm --env-file .env blog /app/blog migrate
docker run -d -p 8000:8000 --env-file .env -v blog-storage:/app/storage blog
```

### Reverse proxy (TLS)

App phục vụ HTTP; TLS kết thúc ở proxy. Ví dụ với Caddy:

```text
blog.example.com {
    reverse_proxy 127.0.0.1:8000
}
```

- Đặt `APP_URL=https://blog.example.com`, để cookie có cờ `Secure`.
- Khai báo IP của proxy trong `http.trusted_proxies` (ví dụ `["127.0.0.1/32"]`),
  để rate limit thấy đúng IP người dùng. Không khai báo proxy thì mọi người dùng
  chung một IP, và bị giới hạn chung.
- `http.body_limit` (mặc định 1 MiB) và giới hạn upload (`[storage]`) phải nhỏ
  hơn hoặc bằng giới hạn body của proxy.

## 7. Nhiều instance

1. Build với feature `redis`, rồi đặt `REDIS_URL` (một Redis standalone, không
   phải Cluster).
2. Mọi instance dùng **cùng** `APP_KEY`, `DATABASE_URL` và `redis.prefix`.
   Hai app khác nhau trên cùng một Redis phải có prefix khác nhau.
3. Upload phải nằm ở chỗ dùng chung: S3 (feature `s3`), hoặc một volume mà
   mọi instance cùng mount.
4. Chỉ một nơi chạy `migrate` mỗi lần deploy, trước khi các instance mới khởi động.

Khi đó:
- rate limit được đếm chung ở Redis;
- job định kỳ (dọn session, xoá media) dùng lease Redis để giảm chạy trùng; job vẫn có thể chạy đồng thời khi lease hết hạn hoặc Redis lỗi, nên phải an toàn khi chạy lặp;
- quyền đổi trên một instance có hiệu lực trên mọi instance từ request sau.

Không có Redis thì chỉ nên chạy **một** instance. Chi tiết ở ADR 0012 và
[Production checklist](../../guide/src/security/production.md).

## 8. Checklist nhanh

- [ ] `APP_ENV=production`, `APP_DEBUG=false`, `APP_KEY` lưu như bí mật.
- [ ] `APP_URL` là URL https công khai; `auth.admin_origins` chỉ chứa origin của admin.
- [ ] `http.trusted_proxies` chỉ chứa IP của proxy/load balancer.
- [ ] Đã chạy `migrate` và `permission:sync` sau mỗi lần deploy.
- [ ] Đã backup database và `storage/app`.
- [ ] Load balancer health check gọi `/health`.
- [ ] Toàn bộ checklist bảo mật: [Production checklist](../../guide/src/security/production.md).
