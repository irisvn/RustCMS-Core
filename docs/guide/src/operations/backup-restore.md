# Backup and restore

A backup is three things, taken at the same point in time:

1. **The database** (`pg_dump`, `mysqldump --single-transaction`, or the
   SQLite file with `sqlite3 database.sqlite ".backup backup.sqlite"`).
2. **The storage** (the `storage.root` folder, or the bucket: turn on bucket
   versioning so a snapshot can be read as of a time).
3. **The keys**: `APP_KEY` and every key in `APP_PREVIOUS_KEYS` at the time
   (during an unfinished [rotation](app-key.md), the database holds values
   under several keys), kept apart from both. Without them, saved secrets
   (the SMTP password, other settings secrets) and your
   [encrypted columns](../digging-deeper/encryption.md) cannot be decrypted,
   and sessions and links stop working.

## Taking a consistent backup

Files referenced by the database may be deleted while a live backup runs
(pruning, media removal). Either:

- stop the web and worker processes, dump the database, copy the storage,
  start again; or
- dump the database, and restore the storage *as of the dump's time* from
  the bucket's versions (versioning keeps deleted objects for a while).

## Keys and rotated backups

Encrypted values open with `APP_KEY` or with a key in `APP_PREVIOUS_KEYS`.
To restore a backup taken before a [key rotation](app-key.md), restore with
the whole set of keys the backup needs: either the backup's keys as they
were (`APP_KEY` and `APP_PREVIOUS_KEYS`), or keep the current key as
`APP_KEY` and list every key of the backup in `APP_PREVIOUS_KEYS` (never the
current key itself: boot fails). Then run `rustcms key:reencrypt`, with that
same environment, to move the restored values to the current key. A value
no key you hold opens cannot be recovered: `key:reencrypt` lists it as
unreadable (check first that the command ran with every key you have), and
it must be set again.

## Restoring

1. Stop every web and worker process.
2. Restore the database and the storage of the same point in time, with the
   keys that sealed them (see above).
3. Before opening, end what the backup brought back: every session and JWT
   sign-in (a sign-in revoked after the backup would work again), links of
   resets and invitations, and mail jobs that were waiting:

   ```sql
   DELETE FROM sessions;
   DELETE FROM jwt_refresh_tokens;
   DELETE FROM jwt_families;
   DELETE FROM user_tokens;
   DELETE FROM jobs WHERE kind IN ('auth.password_reset', 'auth.invitation', 'mail.send');
   ```

   Revoke API tokens too if the backup may hold some that were revoked
   since:

   ```sql
   UPDATE personal_access_tokens SET revoked_at = CURRENT_TIMESTAMP WHERE revoked_at IS NULL;
   ```

4. **Check every other job the backup brought back** before any worker
   starts (`SELECT id, kind, queue, created_at, description FROM jobs`):
   your own jobs, exports, imports and `rustcms.schedule.run` (a Run now of
   a task schedule). Any of them may already have run after the backup was
   taken, and the restored `job_receipts` predate those runs, so `queue::once`
   does not stop them from running again. Delete the jobs whose effects
   already happened outside the database (a mail, a payment, a call to
   another service).
5. `rustcms migrate:status`, then `rustcms migrate` if the code is newer.
   If migrations show as `Newer`, the database was written by a newer binary:
   restore with that binary (or a newer one). `rustcms migrate --allow-newer`
   leaves those migrations as they are and runs this version's pending ones;
   use it only after checking that every unknown migration only adds tables
   or nullable columns, so the older code still works with the schema.
6. Start the processes; users sign in again.

A missed schedule slot is skipped, never caught up, so the scheduler tables
(`schedule_states`, `schedule_slots`, `schedule_runs`) need nothing special;
that does not cover jobs already in the `jobs` table (step 4).

Rehearse a restore on a copy: check that files open, that sign-in works, and
that `rustcms key:reencrypt --dry-run`, run with the restored environment,
succeeds (every encrypted value opens with the current key).

## Related

- [Rotating APP_KEY](app-key.md)
- [Data retention](retention.md)
- [File storage](../digging-deeper/storage.md)
- [Database schema](../reference/database-schema.md)
