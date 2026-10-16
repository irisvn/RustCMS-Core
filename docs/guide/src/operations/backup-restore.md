# Backup and restore

A backup is three things, taken at the same point in time:

1. **The database** (`pg_dump`, `mysqldump --single-transaction`, or the
   SQLite file with `sqlite3 database.sqlite ".backup backup.sqlite"`).
2. **The storage** (the `storage.root` folder, or the bucket: turn on bucket
   versioning so a snapshot can be read as of a time).
3. **`APP_KEY`**, kept apart from both: without it, saved secrets (the SMTP
   password) cannot be decrypted, sessions and links stop working.

## Taking a consistent backup

Files referenced by the database may be deleted while a live backup runs
(pruning, media removal). Either:

- stop the web and worker processes, dump the database, copy the storage,
  start again; or
- dump the database, and restore the storage *as of the dump's time* from
  the bucket's versions (versioning keeps deleted objects for a while).

## Restoring

1. Stop every web and worker process.
2. Restore the database and the storage of the same point in time, with the
   same `APP_KEY`.
3. Before opening, end what the backup brought back: every session and
   sign-in token issued before the backup, links of resets and invitations,
   and jobs that were waiting:

   ```sql
   DELETE FROM sessions;
   DELETE FROM user_tokens;
   DELETE FROM jobs WHERE kind IN ('auth.password_reset', 'auth.invitation', 'mail.send');
   ```

   (Revoke API tokens too if the backup may hold some that were revoked since.)
4. `rustcms migrate:status`, then `rustcms migrate` if the code is newer.
5. Start the processes; users sign in again.

Rehearse a restore on a copy: check that files open and that sign-in works.
