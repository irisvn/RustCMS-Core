# Example: an LMS back office

`examples/lms` is a small learning-management back office: courses, lessons,
learners and enrollments, run by staff and instructors. It follows the
[application layout](../getting-started/directory-structure.md) and shows
where each kind of code goes.

```sh
cd examples/lms
cp .env.example .env && cargo run -- key:generate
cargo run -- migrate && cargo run -- db:seed && cargo run -- serve
```

Sign in at `/admin` (password `password123`) as `staff@example.test` (every
course) or `teacher@example.test` (their courses only). `db:seed` completes
what an earlier run left and refuses to run in production: it creates
accounts with a public password.

## Data

| Table | Columns |
|---|---|
| `courses` | title, slug (unique), summary, capacity, published, `instructor_id` → users |
| `lessons` | `course_id` → courses (deleted with it), title, position, body |
| `learners` | name, email (unique), phone: people taking courses, without a sign-in |
| `enrollments` | `course_id`, `learner_id` (unique together, deleted with either), status `active` / `withdrawn`, progress 0–100 |

## Where the code is

| Path | Holds |
|---|---|
| `src/services/enrollment_service.rs` | the seat rules (`check`, `enroll_in`, `enroll`), used by the panel, the enroll action and the seeders |
| `src/models/course.rs`, `learner.rs` | query scopes: `course::taught_by`, `learner::taught_by` |
| `src/policies/` | `permissions.rs` (names), `enrollment_policy.rs` |
| `src/admin/resources/` | the four resources: fields, forms, tables, scopes, hooks that call the service |
| `src/settings/lms_settings.rs` + `src/admin/pages/lms_settings_page.rs` | the `lms` settings group and its page |
| `src/jobs/send_welcome.rs` | the `lms.enrolled` job |
| `src/database/seeders/` | roles, users, courses, learners, enrollments |
| `src/database/factories/` | courses and learners for tests |
| `tests/feature/` | enrollments, instructors, seeding, languages |

## Seats

A course takes at most `capacity` active enrollments, and a learner enrolls
in a course once (withdrawing and coming back change the same row). The
rules are one function in the service:

```rust,ignore
// src/services/enrollment_service.rs
pub async fn check(txn, course_id, learner_id, status, current) -> AppResult<Result<(), Refusal>>
// AlreadyEnrolled (said before the seats), NoSeat, NoCourse, UnknownStatus
```

It counts the course's enrollments, so it runs under the course's row lock:

- in the panel, the enrollment resource declares
  `.lock_parent("course_id", "courses")`: every enrollment write locks its
  course first (both courses when it moves), then the hooks call `check`
  ([parent locks](../admin/resources.md#parent-locks));
- the course's **Enroll a learner…** action (staff: `enrollments.create`)
  asks for a learner in a dialog; the action locks the course, then calls
  `enrollment_service::enroll_in(txn, …)` in its transaction. A refusal
  (already enrolled, no seat) answers 422 in the dialog and nothing is kept;
- elsewhere, `enrollment_service::enroll` opens its own transaction, locks
  the course and calls `enroll_in`. The enrollment seeder uses it, so seeding
  never overfills a course.

Concurrent requests for the last seat get one 201 and one 422; a course and
its enrollments changed at the same time never deadlock (the course is
always locked first, the order its deletion cascades in). A capacity below
the active enrollments is refused. The unique constraint
`enrollments_course_learner_key` stays as the last line of defense, mapped to
a message with `ResourceMeta::unique`.

To move an enrollment, change its `course_id` (staff): the seat is checked
in the new course. To withdraw, set `status` to `withdrawn`; setting it back
to `active` needs a free seat.

## Instructors

Staff hold `lms.all_courses`; everyone else sees what they teach. Each
resource's `scope()` adds an `EXISTS` subquery from the model (a join could
repeat rows):

```rust,ignore
// src/admin/resources/lesson_resource.rs
fn scope(query: Select<lesson::Entity>, ctx: &Ctx) -> Select<lesson::Entity> {
    match instructor(ctx) {
        None => query,
        Some(me) => query.filter(course::taught_by(me, (lesson::Entity, lesson::Column::CourseId))),
    }
}
```

The scope applies to lists, direct ids, options, relations and writes:
another teacher's course, its lessons, enrollments and learners answer 404,
are not offered as options, and cannot be picked for a lesson (also not by
overriding `course_id` when creating from a course's page).

Field rules and the policy do the rest:

- `instructor_id` is set only with `courses.assign_instructor`, and must name
  a user. Lessons and enrollments lock their course first, so an instructor
  removed from a course loses access to it at once.
- `progress` is never set when enrolling and changes only with
  `enrollments.update_progress`. `EnrollmentPolicy` lets that permission
  update an enrollment; the other fields stay closed to it. It also refuses
  abilities before any lookup (`admits`: 403 whether an enrollment exists or
  not) and never filters records itself (`view_depends_on_record() == false`:
  lists need no scan), since the scope decides visibility.

## Settings, job, languages

- The `lms` group (school name, default capacity of seeded courses) is edited
  at `/admin/settings/lms` by staff (`settings.lms.view` / `.update`).
- A new active enrollment queues `lms.enrolled` with `ctx.dispatch(txn, …)`
  in its own transaction: a refused or failed enrollment queues nothing. The
  job carries ids only and logs a welcome with the school name.
- `lang/vi.toml` translates labels, the service's messages (through
  `ctx.t`) and the unique constraint messages; a test checks that
  `lang:missing vi` has nothing to report.

## The panel's look and extras

- **Theme**: blue primary color (lighter in dark mode), rounder corners, a
  logo and a favicon from `resources/admin/`.
- **Course overview** (`src/admin/pages/course_overview_page.rs`, template
  `resources/views/admin/course_overview.html`): each course the user may
  list, with a meter of seats taken. It reads courses through
  `ctx.records("courses", …)`, so instructors see theirs only.
- **Seats** beside a course (`src/admin/slots/course_seats_slot.rs`): active
  and withdrawn enrollments, average progress.
- **Progress cell** (`resources/admin/js/lms/progress.js`): the enrollments
  table draws progress as a bar (a native `meter`, styled by
  `resources/admin/css/lms/progress.css`).

See [Customizing the panel](../admin/customizing.md).

## Conflicts

A request that met another change to the same data answers 409
`write_conflict` with nothing saved; send it again. The panel offers to.

## Tests

```sh
cargo test -p lms                      # SQLite
RUSTCMS_TEST_PG=postgres://postgres:postgres@localhost:55439/postgres \
RUSTCMS_TEST_MYSQL=mysql://root:mysql@localhost:53306/mysql \
cargo nextest run -p lms               # SQLite, PostgreSQL and MySQL
```

Not part of the example yet: learner sign-in, mail (the welcome is logged),
quizzes.
