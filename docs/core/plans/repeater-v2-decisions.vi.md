# Quyết định cho phần 6 của PR #4 (repeater v2, ADR 0044)

Ngày 2026-11-22. Anh Đức giao em quyết các mục Q1–Q7 (và C, D, E) dựa trên review thiết kế của Codex. Báo cáo
review: `docs/core/reports/2026-11-22-pr4-review.vi.md`, mục "Codex duyệt lại thiết kế".

Nguyên tắc chung khi quyết:
- Khi phân vân, chọn phương án an toàn hơn: giữ dữ liệu, không mở thêm đường lộ quyền.
- Giữ API công khai nhỏ. Thêm vào thì dễ, gỡ ra thì khó.
- Không lật lại quyết định cũ (ADR 0018, 0041, 0043) nếu chưa có lý do mới thật sự.

## Tóm tắt dễ đọc

| Mục | Tác giả đề xuất | Em quyết | Nói đơn giản |
|---|---|---|---|
| Q1 | Ẩn một repeater quan hệ thì xóa các dòng của nó khi lưu | **Không làm** | Ẩn chỉ là ẩn: dữ liệu vẫn còn. App nào cần xóa thì tự xóa trong hook của mình |
| Q2 | Lồng repeater 3 tầng | **Chưa làm** (gác phần 6C) | Chờ có ứng dụng thứ hai cần. Tạm thời sửa tầng thứ 3 ở trang riêng của nó |
| Q3 | Relation có lọc (`where_null`, `where_eq`) | **Làm, kèm sửa** | Người dùng phải được đọc ô lọc mới thấy relation đó, và phải được ghi ô đó mới tạo được bản ghi qua relation |
| Q4 | Rich text gọn trong dòng | **Làm** | Chỉ là giao diện; server vẫn kiểm tài liệu như cũ |
| Q5 | Closure chạy trên server làm sau cùng | **Chỉ làm `options_fn`** | Danh sách lựa chọn phụ thuộc ô khác (topic theo subject). Chạy trước khi khóa dữ liệu. Không làm `shown_when_fn`/`required_fn` |
| Q6 | Chấp nhận breaking change | **Không chấp nhận chung chung** | Cái nào giữ tương thích được với chi phí thấp thì giữ; cái còn lại ghi rõ trong upgrade guide |
| Q7 | Server tự điền khóa của tổ tiên (`inherit`) | **Gác cùng phần 6C** | Chỉ cần khi có lồng 3 tầng |

## Chi tiết

### Q1: ẩn repeater thì xóa dòng → không làm
- Codex: quy tắc của form (ADR 0040) là mọi thay đổi trên dòng phải do người dùng chủ động gửi lên. Tự xóa khi
  một ô khác đổi giá trị đi ngược quy tắc đó.
- Tự xóa còn có thể xóa cả những dòng người khác vừa thêm sau khi trang được mở.
- Hành vi này chạy cả khi import CSV và gọi API, tức là có thể xóa dữ liệu mà không ai nhìn thấy.
- **Hệ quả cho app:** đổi loại câu hỏi thì các đáp án cũ vẫn còn trong database, chỉ không hiện ra. Nếu ứng dụng
  thật sự cần dọn đi, viết trong hook `mutate_before_update`/`after_save` của chính app.
  Sau này nếu cần, có thể làm thành tùy chọn bật riêng cho từng repeater, chỉ áp cho các dòng đã hiện trên form.

### Q3: relation có lọc → làm, kèm sửa
Quy tắc mới (thay câu "the user needs no write rule on the field" trong ADR):
- **Đọc:** người dùng phải đọc được mọi ô lọc. Không đọc được thì relation biến khỏi schema, và mọi đường đọc
  (danh sách, số đếm, dòng trên trang xem, repeater) đều từ chối.
- **Tạo:** phải ghi được (`can_write` khi tạo) mọi ô mà relation tự điền.
- **Sửa:** quyền sửa như bình thường. Hook không được làm bản ghi rời khỏi relation, kể cả bằng cách bỏ ô lọc
  khỏi dữ liệu.
- **Kiểu dữ liệu:** chỉ cho lọc trên chuỗi, số nguyên, boolean, uuid, ngày (boot từ chối kiểu khác). Decimal so
  bằng giá trị đã chuẩn hóa. Hai relation phải rời nhau trên cả MySQL (không phân biệt hoa thường).
- Sửa thêm: ô lọc `required` không làm hỏng mục tạo mới; số đếm khi policy phụ thuộc bản ghi tính cả bộ lọc.

### Q5 / phần E: closure trên server → chỉ `options_fn`
- Chạy **trước** khi mở transaction và khóa bản ghi, dưới cùng giới hạn đồng thời và tần suất của reaction
  (ADR 0043). Không bao giờ chạy code của app trong lúc giữ khóa.
- Không có route `form/rules` trả mọi luật. Thay bằng yêu cầu lấy options cho **một** ô người dùng có trong form.
- Giới hạn số options và kích thước trả về.
- Khi lưu: giá trị phải nằm trong danh sách đã tính (không kiểm thêm danh sách khai báo tĩnh).
- Bỏ `shown_when_fn` và `required_fn`: đã có `when(..)` + `required`, và hook validate của app.

### Q6: breaking change → xét từng cái
Giữ tương thích:
- Điều kiện của form con vẫn áp cho dòng như 0.13. Điều kiện riêng của repeater là khai báo mới, tường minh.
- `Condition` không đổi hình dạng: phạm vi `root`/`parent` đi qua một wrapper mới.
- `ListQuery` không có trường public mới: bộ lọc relation đi trong context nội bộ.
- `DynResource` giữ các method cũ.

Không tránh được (ghi trong upgrade guide):
- biến thể mới của `FormPatch` cho reaction trên dòng;
- struct schema có thêm trường.

### Phần B (file, rich text, `belongs_to` trong dòng) → làm, kèm sửa
- Lập luận về khóa đúng: media kiểm một lần, cuối cùng, theo thứ tự id. Khóa ngầm qua khóa ngoại vẫn rơi vào hợp
  đồng 409 của ADR 0041.
- Sửa thêm:
  - giới hạn tổng số media mỗi lần ghi, chia lô khi vượt 500 (không trả 422 sai chỗ);
  - lỗi chỉ đúng dòng;
  - `after_save` của dòng chạy **sau** khi media đã kiểm xong.

### Phần D
- **Reaction sửa dòng quan hệ:** làm, kèm sửa. Ô phải đọc **và** ghi được (lỗi lộ dữ liệu Codex tìm thấy), quyền
  theo từng dòng, giới hạn của reaction.
- **`validate_rows`:** **không làm dạng hiện tại.** Nó đưa cho code app mọi dòng với mọi field, kể cả dòng người
  dùng không được xem, nên thành một API đặc quyền. Nó cũng không giữ được bất biến thật: đổi riêng bản ghi cha,
  hoặc ghi thẳng vào bảng con, đều đi vòng qua nó.
- Bất biến kiểu "câu một đáp án chỉ có một đáp án đúng" viết trong hook hoặc service của app.

### Phần C (lồng 3 tầng, `inherit`, chuỗi khóa) → gác lại
- Codex chỉ ra lập luận khóa chưa đúng:
  - `lock_parents` chỉ khóa cha trực tiếp;
  - phần ghi đệ quy chỉ sắp thứ tự trong từng tầng.

  Vì vậy chưa có thứ tự khóa toàn cục.
- Giữ nguyên "no chains" của ADR 0018.
- Làm lại khi có ứng dụng thứ hai cần, với một thiết kế khóa và vòng đời (xóa, xóa mềm, khôi phục) đầy đủ.

## Lộ trình

1. **0.13.2:** sửa bảo mật dependency. Xong, nhánh `fix/security-0.13.2`.
2. **0.14.0:** phần 1–5 của PR. Xong, nhánh `integrate/pr4-0.14`.
3. **0.14.0 (anh Đức chọn gộp một bản release, 2026-11-23):** phần A (theo Q1, Q3, Q6 ở trên, gồm cả bảng và dòng trên trang xem), phần B. Reaction trên dòng và
   `options_fn` rút gọn dời sang bản sau, mỗi phần có thiết kế và Codex riêng (kế hoạch v2.1). Viết lại ADR 0044 thành quyết định thật: bỏ ghi "taken with the owner" và khôi phục
   hợp đồng của ADR 0018, 0041, 0043.
4. **Chưa xếp lịch:** phần C, `inherit`, `validate_rows`.
