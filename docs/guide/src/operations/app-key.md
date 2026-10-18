# Rotating APP_KEY

`APP_KEY` keys:
- session ids, JWT signatures and refresh tokens;
- the links of password resets and invitations;
- the secrets of the settings and the [encrypted columns](../digging-deeper/encryption.md).

Rotate it when it may have leaked.

What a new key does:

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

1. `rustcms key:generate --show` gives the new key.
2. In the environment of **every** web and worker process:
   - `APP_KEY` = the new key;
   - `APP_PREVIOUS_KEYS` = the old key (comma-separated if several).
3. Stop **every** process, then start them all. A rolling restart does not
   work: a process still on the old key cannot read what the new ones seal.
4. Run `rustcms key:reencrypt` until it reports 0 to move, 0 changed and 0
   unreadable. It exits non-zero until then, and running it again is safe.
   - Unreadable rows are listed by table, column and key. They were sealed
     with a key you no longer have: set those secrets again.
   - `--list` shows the columns and settings it covers.
5. Remove `APP_PREVIOUS_KEYS`, restart, and check with
   `rustcms key:reencrypt --dry-run`: it must report 0 unreadable.
6. Revoke API tokens if needed:
   - each user's *Sign out other sessions* (Account page) revokes theirs;
   - or set `revoked_at` on the rows of `personal_access_tokens`.

Old backups still open with the old key: keep or destroy them accordingly.
