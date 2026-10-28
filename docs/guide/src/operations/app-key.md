# Rotating APP_KEY

`APP_KEY` keys:
- session ids, JWT signatures and refresh tokens;
- the links of password resets and invitations;
- the secrets of the settings and the [encrypted columns](../digging-deeper/encryption.md).

Rotate it when it may have leaked. This page is the procedure: a restart of
every process, a re-encryption run, a verification run, and a second restart.

## What a new key does

| Item | After rotation |
|---|---|
| Admin sessions | ended (everyone signs in again) |
| JWT access and refresh tokens | ended |
| Reset and invitation links | dead (ask for new ones; send invitations again) |
| Secret settings, encrypted columns | still readable while the old key is in `APP_PREVIOUS_KEYS`; `key:reencrypt` moves them to the new key |
| Personal API tokens (`rcms_…`) | **kept** (not keyed by `APP_KEY`): revoke them if they may have leaked too |

The old key in `APP_PREVIOUS_KEYS` only opens encrypted values. It never
brings back a session, a token or a link.

## Steps

1. Generate the new key:

   ```sh
   rustcms key:generate --show
   ```

   `--show` prints it. Without it, the command writes `APP_KEY` to `.env`,
   and refuses to replace an existing one unless you add `--force`.
2. In the environment of **every** web and worker process, **and of the
   shell, job or container where you run the `rustcms` commands below**
   (each command reads its own configuration):
   - `APP_KEY` = the new key;
   - `APP_PREVIOUS_KEYS` = the old key, plus any older keys still listed there
     (comma-separated).

   A command run with the old environment checks the old key: its results
   prove nothing about the new one.
3. Stop **every** process, then start them all. A rolling restart does not
   work: a process still on the old key cannot read what the new ones seal.
4. Run `rustcms key:reencrypt`. It re-seals every registered encrypted column
   and every settings secret with the new key, and prints one line per column:

   ```text
   integrations.api_token: 0 current, 12 moved, 0 changed meanwhile, 0 unreadable
   settings: 3 current, 1 moved, 0 changed meanwhile, 0 unreadable
   ```

   It fails (non-zero exit) only when a row changed meanwhile or a value is
   unreadable; rows it moved count as done. Running it again is safe.
   - **Changed meanwhile:** someone wrote the row during the run. Run the
     command again.
   - **Unreadable:** listed by table, column and key (or settings group).
     No key of this command's keyring opens them. First check that the
     command runs with the new `APP_KEY` and every previous key you still
     have, and that a column registered with `Binding::Row` is really sealed
     with `.row(id)` (and the reverse). A value also reads as unreadable when
     it is damaged. Set a secret again only once you know no key you hold
     opens it.
   - `--list` shows the columns and settings it covers; `--batch <n>` (1 to
     10000, default 500) sets how many rows it reads at a time.
5. Check, in the same environment (new `APP_KEY`):
   `rustcms key:reencrypt --dry-run` must succeed. A dry run fails while any
   value is still to move, so success means everything is under the new key.
6. Remove `APP_PREVIOUS_KEYS` everywhere and restart every process. Keep the
   old key with the backups that still need it ([below](#backups)).
7. Revoke API tokens if needed:
   - each user's *Sign out other sessions* (Account page) ends their other
     sessions and revokes their tokens;
   - or set `revoked_at` on the rows of `personal_access_tokens`:

     ```sql
     UPDATE personal_access_tokens SET revoked_at = CURRENT_TIMESTAMP WHERE revoked_at IS NULL;
     ```

## Setting the keys

| Variable | Config key | Form |
|---|---|---|
| `APP_KEY` | `app.key` | the key `key:generate` prints |
| `APP_PREVIOUS_KEYS` | `app.previous_keys` | comma-separated keys, or a TOML list in `config/app.toml` |

Both also accept a `_FILE` variant (`APP_KEY_FILE`,
`APP_PREVIOUS_KEYS_FILE`) that reads the value from a file, for Docker and
Kubernetes secrets ([Configuration](../getting-started/configuration.md)).

Boot fails when `APP_PREVIOUS_KEYS` lists a key twice or lists the current
`APP_KEY`. Errors never quote a key.

## Backups

Old backups hold values sealed with the old key, and a backup taken during
an unfinished rotation can hold values under several keys. They open again
only with those keys, as `APP_KEY` or in `APP_PREVIOUS_KEYS`. Keep every key
a backup needs with it, or destroy both
([Backup and restore](backup-restore.md)).

## Related

- [Encrypted columns](../digging-deeper/encryption.md)
- [Revocation](../security/revocation.md)
- [Production checklist](../security/production.md)
