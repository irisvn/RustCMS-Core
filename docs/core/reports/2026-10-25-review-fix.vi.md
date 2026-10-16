# Báo cáo: sửa theo Codex review toàn bộ M17b-fix..M22

> Ngày: 25/10/2026 · trên `6c92e8f`. Codex: 1 P1, 6 P2, 4 P3.

## Đã sửa
- **P1** Import tạo bản ghi sau khi quyền `create` bị thu hồi (còn `update`) → kiểm `create` bằng `wctx` (quyền
  hiện tại, dưới khoá RBAC) trước hook/insert. Test hồi quy.
- **P2** Request reset cũ hồi sinh sau khi token mới được dùng → dùng token / admin đặt mật khẩu để lại "mốc"
  mỗi mục đích (không secret nào khớp, giữ 30 ngày); request cũ hơn bị coi là đã thay thế. `invitation_days` ≤ 30.
  Test `A → B → dùng B → retry A` và sau khi admin đặt mật khẩu.
- **P2** Upload import bị cắt sau commit trước khi lưu file → lưu file trước, rồi mới commit import + receipt;
  lỗi/bị từ chối thì xoá object.
- **P2** `write_conflict` khi import (chờ khoá RBAC) làm hỏng dòng → thử lại cả job (receipt giữ mỗi dòng một lần).
- **P2** Thống kê so sánh trên trường Date lệch giữa SQL và duyệt → mọi biên dời tới nửa đêm kế tiếp (UTC) ở cả hai.
- **P2** Prune export xoá bản ghi object khi upload còn chạy → job không chạy khi đã hết hạn; prune chờ 1 giờ sau hạn.
- **P2** Trang reset/mời mất phản hồi → báo "có thể mật khẩu mới đã dùng được", không cho gửi lại. Vitest.
- **P3** Điều hướng sau khi rời trang → guard `mounted`.
- **P3** Dashboard không làm mới sau khi ghi → invalidate `["dashboard"]` sau mọi lần ghi.

## Chuyển sang M23
- **P3** Fixture SMTP TLS (chứng chỉ sai tên, AUTH qua TLS, reply chứa secret) — cần dựng TLS server test.
- **P3** Test nâng cấp 0.2 với thông tin đăng nhập thật và DDL MySQL dở dang — thuộc phạm vi M23.
