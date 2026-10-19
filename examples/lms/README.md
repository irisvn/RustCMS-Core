# lms

A minimal learning-management back office built with rustcms: courses with a
capacity and an instructor, their lessons, learners (people, not users) and
enrollments. It shows the patterns an LMS needs:

- **Seats under a lock**: the rules live in `src/services/enrollment_service.rs`;
  every enrollment write locks its course first (`lock_parent`), so the last
  seat goes to one request only, and the seeders go through the same service.
- **Instructors see their courses only**: `scope()` with `EXISTS` subqueries
  (from `src/models/`) on courses, lessons, enrollments and learners; staff
  hold `lms.all_courses`.
- **Field rules**: only `courses.assign_instructor` sets a course's
  instructor; `enrollments.update_progress` records progress and nothing else.
- **A job in the enrollment's transaction** (`lms.enrolled`), a settings
  group (`lms`), Vietnamese labels and messages (`lang/vi.toml`).

```sh
cp .env.example .env && cargo run -- key:generate   # first time only
cargo run -- migrate
cargo run -- db:seed       # admin@, staff@, teacher@, tutor@example.test, password `password123`
cargo run -- serve         # http://localhost:8000/admin (also runs the queue workers)
```

The layout is the one `rustcms new` generates (see the guide's "Directory
structure" and "Example: an LMS back office").

Tests: `cargo test` (SQLite; PostgreSQL with `RUSTCMS_TEST_PG`, MySQL with
`RUSTCMS_TEST_MYSQL`). Learner sign-in and quizzes are not part of this
example yet.
