# Rotating APP_KEY

`APP_KEY` keys session ids, JWT signatures and refresh tokens, the links of
password resets and invitations, and encrypts the secrets of the settings.
Rotate it when it may have leaked.

What a new key does:

| Item | After rotation |
|---|---|
| Admin sessions | ended (everyone signs in again) |
| JWT access and refresh tokens | ended |
| Reset and invitation links | dead (ask for new ones; send invitations again) |
| Secret settings (SMTP password, API keys) | **cannot be decrypted**: shown as *Cannot be read*, must be set again |
| Personal API tokens (`rcms_…`) | **kept** (not keyed by `APP_KEY`): revoke them if they may have leaked too |

## Steps

1. Note every secret setting you will need to enter again.
2. `rustcms key:generate --show` and put the new key in the environment of
   **every** web and worker process (an old process keeps the old key in memory).
3. Stop every process, then start them all with the new key.
4. Enter the secret settings again (Settings → Mail …) and send a test mail.
5. Revoke API tokens if needed: each user's *Sign out other sessions* (Account page) revokes theirs; or set
   `revoked_at` on the rows of `personal_access_tokens`.

Old backups still open with the old key: keep or destroy them accordingly.
