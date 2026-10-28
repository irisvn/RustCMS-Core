# Example: an LMS back office

`examples/lms` is a small learning-management back office built on RustCMS:
courses with a capacity and an instructor, their lessons, learners and
enrollments run by staff and instructors, and the school's news written by
editors. Read it when you want a complete application to copy from: it follows
the [application layout](../getting-started/directory-structure.md) and shows
where each kind of code goes.

![The LMS courses table, signed in as staff](/img/examples/lms-courses-staff.webp)
*The LMS example: the courses table as staff see it (every course).*

What it shows:

| Topic | Where |
|---|---|
| Resources with forms, tables, filters, relations | `src/admin/resources/` |
| Business rules in a service, called from hooks, an action and seeders | `src/services/enrollment_service.rs` |
| Parent locks: every enrollment write locks its course first | `ResourceMeta::lock_parent` in `enrollment_resource.rs` |
| Row scoping: instructors see their courses only | `scope()` + `src/models/course.rs`, `learner.rs` |
| Field rules and a policy | `course_resource.rs`, `src/policies/` |
| An action with a form | *Enroll a learner…* on a course; *Move up* / *Move down* on a lesson |
| A dashboard | `src/providers/admin_panel_provider.rs` |
| A custom page, a slot, a custom cell, a theme | `src/admin/pages/`, `src/admin/slots/`, `resources/` |
| A job that sends mail, queued with the write | `src/jobs/send_welcome.rs` |
| A settings group | `src/settings/lms_settings.rs` |
| Media, rich text, belongs-to-many | `src/admin/resources/news_resource.rs` |
| Translations (Vietnamese) | `lang/vi.toml` |
| Feature tests on SQLite, PostgreSQL and MySQL | `tests/feature/` |

## Quick start

From a checkout of the repository:

```sh
cd examples/lms
cp .env.example .env && cargo run -- key:generate   # first time only
cargo run -- migrate
cargo run -- db:seed
cargo run -- serve                                  # also runs the queue workers
```

Open `http://localhost:8000/admin` (the `listen` address in `config/app.toml`)
and sign in with one of the seeded accounts. Every account's password is
`password123`.

| Account | Role | Sees and does |
|---|---|---|
| `admin@example.test` | super admin (admin guard) | everything in the panel, including System → Users and Roles (invites instructors); not a super admin of the API guard, so it cannot replace other users' passwords or emails |
| `staff@example.test` | `staff` | every course (`lms.all_courses`), assigns instructors, enrolls learners, imports learners, exports enrollments, edits the *School* settings |
| `teacher@example.test` | `instructor` | the courses they teach (*Introduction to Rust*, *Web services with Axum*), their lessons, learners and enrollments; records progress |
| `tutor@example.test` | `instructor` | a second instructor (*Databases with SeaORM*): sign in as both to see the scoping |
| `editor@example.test` | `editor` | the news, categories, tags and their images; nothing of the courses |
| `viewer@example.test` | `viewer` | reads everything of the school, exports enrollments, sees the dashboard; changes nothing |

The accounts and their roles come from the seeders:

<<< @/../../../examples/lms/src/database/seeders/user_seeder.rs#accounts [src/database/seeders/user_seeder.rs]

<<< @/../../../examples/lms/src/database/seeders/role_seeder.rs#role-seeder [src/database/seeders/role_seeder.rs]

`db:seed` can run again: each seeder finds rows by a natural key (an email, a
slug) before creating them, so a second run completes what an earlier one
left. The user seeder refuses to run in production, even with
`db:seed --force`: it creates accounts with a public password.

::: tip Another database
`.env.example` has commented `DATABASE_URL`s for PostgreSQL and MySQL 8.4 (the
MySQL database needs `CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin`).
:::

## Data

| Table | Columns |
|---|---|
| `courses` | `title`, `slug` (unique), `summary`, `capacity`, `published`, `instructor_id` → users (set to null when the user is deleted), `created_at` |
| `lessons` | `course_id` → courses (deleted with it), `title`, `position`, `body` |
| `learners` | `name`, `email` (unique), `phone`, `created_at`: people taking courses, without a sign-in |
| `enrollments` | `course_id`, `learner_id` (unique together, deleted with either), `status` `active` / `withdrawn`, `progress` 0–100, `enrolled_at` |
| `categories`, `tags` | `name` (unique) |
| `news` | `title`, `slug` (unique), `body` (rich text), `cover` (a media item), `category_id`, `author_id`, `published_at` (empty: a draft; in the future: scheduled), `created_at` |
| `news_tags` | the tags of an article (belongs-to-many) |

## Where the code is

| Path | Holds |
|---|---|
| `src/bootstrap/mod.rs`, `providers.rs` | the application: providers, migrations, API routes |
| `src/providers/admin_panel_provider.rs` | the panel: theme, resources, settings pages (School, Mail), dashboard, page, slot, cell script, translations |
| `src/providers/auth_service_provider.rs` | permissions beyond each resource's CRUD abilities |
| `src/providers/app_service_provider.rs` | seeders, the `lms.enrolled` job, mail templates |
| `src/services/enrollment_service.rs` | the seat rules (`check`, `enroll_in`, `enroll`), used by the panel, the action and the seeders |
| `src/models/` | entities; scope helpers `course::taught_by`, `learner::taught_by` |
| `src/policies/` | permission names, `instructor(ctx)`, `EnrollmentPolicy` |
| `src/admin/resources/` | the seven resources |
| `src/admin/pages/` | the *Course overview* page and the *School* settings page |
| `src/admin/slots/course_seats_slot.rs` | the *Seats* card beside a course |
| `src/settings/lms_settings.rs` | the `lms` settings group |
| `src/jobs/send_welcome.rs` | the `lms.enrolled` job |
| `src/routes/api.rs` | the JSON API (`GET /api/v1/ping`, public) |
| `src/database/` | migrations, seeders, factories |
| `resources/` | the logo and favicon, page and slot templates, the cell script and style, mail templates |
| `lang/vi.toml` | Vietnamese labels and messages |
| `tests/feature/` | `enrollments`, `instructors`, `news` (with lessons), `customizing`, `seeding`, `languages` |

## Seats and parent locks

A course takes at most `capacity` active enrollments, and a learner enrolls in
a course once (withdrawing and coming back change the same row). The rules are
one function in the service. It returns `Ok(Err(Refusal))` for a refused
enrollment, so callers decide how to report it:

<<< @/../../../examples/lms/src/services/enrollment_service.rs#check [src/services/enrollment_service.rs]

<<< @/../../../examples/lms/src/services/enrollment_service.rs#refusal [src/services/enrollment_service.rs]

`check` counts the course's enrollments, so it must run under the course's row
lock, or two requests could both take the last seat. Every path takes that
lock first.

**In the panel**, the enrollment resource declares a parent lock. Every
enrollment write locks its course before the hooks run (both courses when an
enrollment moves), and a unique constraint is mapped to a message:

<<< @/../../../examples/lms/src/admin/resources/enrollment_resource.rs#meta [src/admin/resources/enrollment_resource.rs]

The hooks then call `check` in the write's transaction. A new active
enrollment also queues the welcome mail in the same transaction, so a refused
or failed write queues nothing:

<<< @/../../../examples/lms/src/admin/resources/enrollment_resource.rs#hooks [src/admin/resources/enrollment_resource.rs]

**In an action**, the course's *Enroll a learner…* asks for a learner in a
dialog. An action locks the records it runs on, so it calls `enroll_in`
directly. A refusal on the learner shows under the field; others show on the
dialog; the whole action rolls back:

<<< @/../../../examples/lms/src/admin/resources/course_resource.rs#actions [src/admin/resources/course_resource.rs]

![The Enroll a learner dialog refusing a full course](/img/examples/lms-enroll-no-seat.webp)
*The LMS example: Enroll a learner… on a full course answers "has no seats left".*

<<< @/../../../examples/lms/src/services/enrollment_service.rs#enroll-in [src/services/enrollment_service.rs]

**Elsewhere** (seeders, jobs, an API), `enroll` opens its own transaction,
locks the course and calls `enroll_in`. The enrollment seeder uses it, so
seeding never overfills a course:

<<< @/../../../examples/lms/src/services/enrollment_service.rs#lock-course [src/services/enrollment_service.rs]

<<< @/../../../examples/lms/src/services/enrollment_service.rs#enroll [src/services/enrollment_service.rs]

The result: two requests for the last seat get one 201 and one 422, and a
course and its enrollments changed at the same time never deadlock (the course
is always locked first, the order its deletion cascades in). The test:

<<< @/../../../examples/lms/tests/feature/enrollments.rs#last-seat [tests/feature/enrollments.rs]

More rules:

- A capacity below the active enrollments is refused (`course_resource.rs`,
  under the same lock: the engine locks a course while it is updated).
- The unique constraint `enrollments_course_learner_key` stays as the last line
  of defence and is answered with the message given to `ResourceMeta::unique`.
- To move an enrollment, staff change its `course_id`: the seat is checked in
  the new course. To withdraw, set `status` to `withdrawn`; setting it back to
  `active` needs a free seat.
- `status` is checked by the service (`UnknownStatus`): the
  `SelectInput::options` of the form do not bound the stored value
  ([options are not validated](../admin/forms.md#options-are-not-validated)).

See [Parent locks](../admin/resources.md#parent-locks) and
[Actions with a form](../admin/actions.md#actions-with-a-form).

## Instructors see their courses

Staff hold `lms.all_courses`; everyone else sees what they teach. One helper
says whose courses bound a user:

<<< @/../../../examples/lms/src/policies/mod.rs#instructor [src/policies/mod.rs]

Courses filter on their own column; lessons, enrollments and learners use an
`EXISTS` subquery from the model (a join could repeat rows):

<<< @/../../../examples/lms/src/admin/resources/course_resource.rs#scope [src/admin/resources/course_resource.rs]

<<< @/../../../examples/lms/src/models/course.rs#taught-by [src/models/course.rs]

<<< @/../../../examples/lms/src/admin/resources/lesson_resource.rs#scope [src/admin/resources/lesson_resource.rs]

The scope applies on every path: lists, direct ids, options, relations and
writes. Another teacher's course, its lessons and enrollments answer 404, are
not offered as options, and cannot be picked for a lesson (not even by
overriding `course_id` when creating from a course's page). Learners are
shared: an instructor sees every learner enrolled (in any status) in at least
one of their courses, even if the learner also takes other teachers'
courses; a learner with no enrollment in their courses answers 404. Lessons and
enrollments lock their course first, so an instructor removed from a course
loses access to it at once.

![The courses table signed in as an instructor](/img/examples/lms-courses-teacher.webp)
*The LMS example: the same table as `teacher@example.test`, two courses only.*

### Field rules and the policy

The extra permissions are declared on the auth provider:

<<< @/../../../examples/lms/src/policies/permissions.rs#permissions [src/policies/permissions.rs]

<<< @/../../../examples/lms/src/providers/auth_service_provider.rs#permissions [src/providers/auth_service_provider.rs]

- `instructor_id` is set only with `courses.assign_instructor` (field rules
  `creatable` / `updatable`), and must name a user (checked in the hook):

  <<< @/../../../examples/lms/src/admin/resources/course_resource.rs#fields [src/admin/resources/course_resource.rs]

- `progress` is never set when enrolling (`creatable(|_| false)`) and changes
  only with `enrollments.update_progress`. `EnrollmentPolicy` lets that
  permission `update` an enrollment; the field rules keep every other field
  closed to it:

  <<< @/../../../examples/lms/src/policies/enrollment_policy.rs#policy [src/policies/enrollment_policy.rs]

  `admits` refuses abilities before any lookup (403 whether the enrollment
  exists or not). `view_depends_on_record() == false` says the policy never
  filters records itself: the scope decides visibility, so lists need no
  record-by-record scan.

See [Authorization](../admin/authorization.md).

## Lessons in order

*Move up* / *Move down* on a lesson swap it with its neighbour among the
lessons of its course that the user may see. The lesson resource declares
`.lock_parent("course_id", "courses")`, so the action locks the course and
moves of one course never interleave. The lessons are ordered by position
(ties in id order), the lesson swaps places with its neighbour (at either end
the move does nothing), then positions are written as 1…n; every lesson whose
position changes is recorded in the audit log:

<<< @/../../../examples/lms/src/admin/resources/lesson_resource.rs#reorder [src/admin/resources/lesson_resource.rs]

![Lessons with the Move up and Move down actions](/img/examples/lms-lessons-move.webp)
*The LMS example: a course's lessons and their Move up / Move down actions.*

## News

Editors (`editor` role) write articles with a cover image and a rich-text
body, pick a category and link tags on the article's page
(`news.attach_tags` / `news.detach_tags`). An empty slug is made from the title
(`slugify`, Vietnamese marks removed); a taken slug is refused with 422.
*Scheduled* is an article whose `published_at` is in the future: nothing
publishes it, the date only filters it.

![The news editor with a cover and tags](/img/examples/lms-news-editor.webp)
*The LMS example: an article with its cover, rich-text body and tags.*

See [Media, uploads & rich text](../admin/media.md) and
[Relationships](../admin/relationships.md).

## Dashboard

Four widgets, each behind its own permission (`dashboard.<name>`), granted to
staff, instructors and viewers. Instructors' counts follow the enrollments'
scope:

<<< @/../../../examples/lms/src/providers/admin_panel_provider.rs#dashboard [src/providers/admin_panel_provider.rs]

![The LMS dashboard](/img/examples/lms-dashboard.webp)
*The LMS example: learners, new enrollments, enrollments per week, new courses.*

See [Dashboard](../admin/dashboard.md).

## Theme, page, slot and cell

**Theme.** A blue primary color (lighter in dark mode), rounder corners, a
logo and a favicon from `resources/admin/`:

<<< @/../../../examples/lms/src/providers/admin_panel_provider.rs#theme [src/providers/admin_panel_provider.rs]

**Course overview** (`/admin/p/course-overview`, `courses.view_any`): the
first 100 courses the user may list (one page; no paging), each with a meter of seats taken, rendered from
`resources/views/admin/course_overview.html`. It reads courses through
`ctx.records`, so the resource's scope and policy apply and instructors see
theirs only:

<<< @/../../../examples/lms/src/admin/pages/course_overview_page.rs#page [src/admin/pages/course_overview_page.rs]

![The Course overview page](/img/examples/lms-course-overview.webp)
*The LMS example: the Course overview page, seats taken per course.*

**Seats** beside a course: active and withdrawn enrollments and the average
progress:

<<< @/../../../examples/lms/src/admin/slots/course_seats_slot.rs#slot [src/admin/slots/course_seats_slot.rs]

<<< @/../../../examples/lms/resources/views/admin/course_seats.html#template [resources/views/admin/course_seats.html]

![A course page with the Seats card](/img/examples/lms-seats-slot.webp)
*The LMS example: a course's page with the Seats card beside it.*

**Progress cell.** The enrollments table draws `progress` as a bar (a native
`meter` styled by `resources/admin/css/lms/progress.css`):

<<< @/../../../examples/lms/src/admin/resources/enrollment_resource.rs#table [src/admin/resources/enrollment_resource.rs]

<<< @/../../../examples/lms/resources/admin/js/lms/progress.js#cell [resources/admin/js/lms/progress.js]

<<< @/../../../examples/lms/src/providers/admin_panel_provider.rs#cell-extension [src/providers/admin_panel_provider.rs]

![The enrollments table with progress bars](/img/examples/lms-progress-cell.webp)
*The LMS example: enrollments with the `lms/progress` cell.*

See [Theming and branding](../admin/theming.md) and
[Customizing the panel](../admin/customizing.md).

## Mail and the welcome job

A new active enrollment queues `lms.enrolled` in its own transaction (the
hook above, or `enroll_in`). The job carries ids only and reads the rows
again when it runs, sends the `welcome` mail with the school name, then
records a receipt for the course/learner pair, so a later run sends nothing
while the receipt is kept (receipts are deleted after
`queue.failed_retention_days`, 30 by default):

<<< @/../../../examples/lms/src/jobs/send_welcome.rs#job [src/jobs/send_welcome.rs]

It is registered with its templates (`resources/views/mail/{en,vi}/welcome.*`):

<<< @/../../../examples/lms/src/providers/app_service_provider.rs#jobs [src/providers/app_service_provider.rs]

`config/mail.toml` uses `driver = "log"` with `source = "config"`: locally
nothing is sent and the log records each mail (recipients count, message id);
add `log_body = true` (local only) to see the text. A receipt is recorded after the mail,
so an interrupted run (after sending, before the receipt) can send it twice.

The administrator invites instructors from System → Users: they choose their
password from the invitation mail (staff have no user management). With the
`log` driver, the link is in the log only with `log_body = true`.

See [Mail](../digging-deeper/mail.md) and [Queues](../digging-deeper/queues.md).

## Settings

The `lms` group (school name, default capacity of seeded courses) is edited at
`/admin/settings/lms` (*School*) by staff (`settings.lms.view` / `.update`).
The Mail settings page is also registered (`.mail_settings()`), for
administrators:

<<< @/../../../examples/lms/src/settings/lms_settings.rs#settings [src/settings/lms_settings.rs]

<<< @/../../../examples/lms/src/admin/pages/lms_settings_page.rs#validate [src/admin/pages/lms_settings_page.rs]

See [Settings](../admin/settings.md).

## Languages

`config/app.toml` offers English and Vietnamese (`locales = ["en", "vi"]`).
`lang/vi.toml` translates labels, the service's messages (through `ctx.t`) and
the unique-constraint messages; a test checks that `cargo run -- lang:missing
vi` has nothing to report. See [Languages](../admin/i18n.md).

## API

`src/routes/api.rs` declares one public route; `rustcms make:api <Model>` adds
controller routes there:

<<< @/../../../examples/lms/src/routes/api.rs#api-routes [src/routes/api.rs]

See [Routing](../basics/routing.md).

## Conflicts

A request that met another change to the same data answers 409
`write_conflict` with nothing saved; send it again. The panel offers to.

## Tests

The feature tests use `db_test!`, so each runs on SQLite, and on PostgreSQL
and MySQL when their URLs are set:

```sh
cargo test -p lms                                      # SQLite only
RUSTCMS_TEST_PG=postgres://postgres:postgres@localhost:5432/postgres \
RUSTCMS_TEST_MYSQL=mysql://root:secret@localhost:3306/mysql \
cargo test -p lms                                      # all three databases
```

A test signs in as a seeded account and checks the queued mail:

<<< @/../../../examples/lms/tests/feature/enrollments.rs#queued-mail [tests/feature/enrollments.rs]

See [Testing](../basics/testing.md).

## What it leaves out

Learner sign-in and quizzes are not part of the example yet.

## Related pages

- [Directory structure](../getting-started/directory-structure.md)
- [Resources](../admin/resources.md)
- [Authorization](../admin/authorization.md)
- [Actions](../admin/actions.md)
- [Customizing the panel](../admin/customizing.md)
- [Testing](../basics/testing.md)
