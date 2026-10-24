# Error codes

Every validation message the framework writes has a stable code, sent in
`issues` / `formIssues` next to the English message ([Errors](../basics/errors.md)).
Clients translate by code and fall back to the message; parameters are text.
A message that follows none of these templates has the code `custom` and
`params.message` holds it.

| Code | English |
|---|---|
| `required` | `is required` |
| `invalid` | `is invalid` |
| `not_allowed` | `is not allowed` |
| `not_found` | `was not found` |
| `several_matches` | `matches several records` |
| `not_imported` | `could not be imported` |
| `set_by_relation` | `is set by the relation` |
| `taken` | `has already been taken` |
| `confirmation` | `does not match` |
| `empty` | `is empty` |
| `given_twice` | `may only be given once` |
| `not_instant` | `must be a date and time (RFC 3339)` |
| `not_after_since` | `must be after since` |
| `token_invalid` | `is invalid or has expired` (a reset or invitation link) |
| `max_length` | `may not be longer than {max} characters` |
| `max_bytes` | `may not be longer than {max} bytes` |
| `min_length` | `must be at least {min} characters` |
| `too_long` | `is too long` |
| `too_large` | `is too large` |
| `max_items` | `may not have more than {max} items` |
| `list_size` | `must be a list of 1 to {max} items` |
| `max_kib` | `may not be larger than {max} KiB` |
| `max_elements` | `may not have more than {max} elements` |
| `max_images` | `may not have more than {max} images` |
| `too_many_roles` | `has too many roles` |
| `too_many_permissions` | `has too many permissions` |
| `unknown_roles` | `has unknown roles` |
| `integer` | `must be an integer` |
| `integer_string` | `must be an integer string` |
| `number` | `must be a number` |
| `number_range` | `must be a number from 1 to {max}` |
| `positive_integer` | `must be a positive integer` |
| `boolean` | `must be true or false` |
| `string` | `must be a string` |
| `decimal_string` | `must be a decimal string` |
| `uuid` | `must be a UUID` |
| `date` | `must be a date (YYYY-MM-DD)` |
| `local_datetime` | `must be a local date-time (YYYY-MM-DDTHH:MM:SS)` |
| `timestamp` | `must be an RFC 3339 timestamp` |
| `out_of_range` | `is out of range` |
| `between` | `must be between 1 and {max}` |
| `email` | `must be a valid email address` |
| `control_characters` | `may not contain control characters` |
| `name_format` | `must be at most {max} characters, without control characters` |
| `text_object` | `must be an object of text values` |
| `not_option` | `is not one of the options` |
| `unreadable_secret` | `cannot be decrypted; replace or clear it` |
| `empty_key` | `may not have an empty key` |
| `key_length` | `may not have keys longer than {max} characters` |
| `value_length` | `may not have values longer than {max} characters` |
| `token_name` | `must be 1 to 255 characters` |
| `abilities` | `must be at most {max} non-empty names` |
| `jwt_abilities` | `must be 1 to {max} non-empty names, {bytes} bytes in all` |
| `jwt_too_long` | `too long for an access token` |
| `idempotency_key` | must be 1 to 128 letters, digits, `_` or `-` |
| `idempotency_reused` | `was already used for a different request; send a new key for a new change` |
| `cannot_filter` | `cannot be filtered` |
| `cannot_sort` | cannot sort by `{name}` |
| `trashed_value` | must be `with` or `only` |
| `trashed_unsupported` | `is not supported by this resource` |
| `malformed_filter` | malformed `{key}` |
| `unknown_operator` | unknown operator `{op}` |
| `search_too_broad` | `matches too many records to check; refine the search` |
| `too_many_matches` | `Too many matching records to check individually; narrow the search or filters.` |
| `one_record_only` | `Run this action on one record at a time.` |
| `too_many_to_export` | `More than {max} records to export; narrow the search or filters.` |
| `too_large_to_export` | `Too large to export at once; export in the background.` |
| `invalid_parent` | `invalid parent key` |
| `invalid_key` | `invalid key` |
| `unsupported_key` | `unsupported key type` |
| `conflicts` | `conflicts with existing data` |
| `missing_reference` | `references a missing or protected record` |
| `stale` | `This was changed by someone else meanwhile. Reload and try again.` |
| `last_super_admin` | At least one active super admin is required for the `{guard}` guard. |
| `no_super_admin_role` | no super_admin role for guard `{guard}` |
| `unknown_permissions` | `unknown permissions: {list}` |
| `duplicate_file` | `may not list the same file twice` |
| `file_types` | `does not accept {types} files` |
| `multipart` | `must be sent as multipart/form-data` |
| `file_part` | must be the only part, named `file` |
| `file_name_length` | `has a name longer than 255 characters` |
| `unreadable` | `could not be read` |
| `file_type` | `is not an accepted file type` |
| `incomplete_file` | `is not a complete file of its type` |
| `image` | `is not a valid image` |
| `too_many_pixels` | `has too many pixels` |
| `document` | `is not a document` |
| `document_invalid` | `is not a valid document` |
| `document_version` | `is not a valid document (version 1)` |
| `document_depth` | `is nested too deeply` |
| `document_structure` | `has a list item outside a list` / `has a list holding something else than list items` / `has a code block holding something else than plain text` / `has a heading without a level from 2 to 4` / has unexpected properties on `{node}` / has an unknown block `{node}` / has an unknown inline `{node}` / has an unknown mark `{node}` / `has a mark with attributes` / `has a text with the same mark twice` / `has a text with too many marks` / `has a text without text` / `has an empty text` |
| `document_link` | `has a link that is not http, https or mailto` / `has a link with unexpected attributes` |
| `document_image` | `has an image that is not a media item` / `has an image with unexpected attributes` |
| `document_alt` | `has an image description longer than {max} characters` |
| `bad_credentials` | `These credentials do not match our records.` |
| `wrong_password` | `The password is incorrect.` |
| `password_for_email` | `is required to change your email` |
| `invalid_body` | `Failed to deserialize the JSON body into the target type: {detail}` |

## Top-level codes

| `code` | Status | `reason` (409) |
|---|---|---|
| `bad_request` | 400 | |
| `unauthenticated` | 401 | |
| `forbidden`, `csrf_mismatch` | 403 | |
| `not_found` | 404 | |
| `method_not_allowed` | 405 | |
| `invitation_accepted`, `user_inactive`, `mail_disabled` | 409 | `accepted`, `inactive`, `disabled` (invitations) |
| `write_conflict` | 409 | `busy` (another change in progress), `upload_expired`, `receipt_expired`; queue actions: `not_failed`, `not_waiting`, `running` |
| `export_not_ready` | 409 | `pending` (the background export has not finished) |
| `export_stale` | 409 | `stale` (access changed since the export was made: export again) |
| `secret_stale` | 409 | `stale` (an encrypted map changed meanwhile: reload before retrying) |
| `import_started` | 409 | `started` (the import has already started) |
| `payload_too_large` | 413 | |
| `validation_failed` | 422 | |
| `too_many_requests` | 429 | |
| `internal_error` | 500 | |

## Mail

Codes of `rustcms::mail::MailError`: answered by `POST settings/mail/test`
(`{ok: false, code}`) and kept as the `last_error` of failed `mail.send` jobs.

| Code | Retried | Meaning |
|---|---|---|
| `mail_disabled` | no | `[mail] driver = "disabled"` |
| `mail_not_configured` | no | no server or sender saved |
| `mail_policy` | no | the server is outside `allowed_hosts` / `allowed_ports` / `allow_insecure` |
| `mail_settings_unreadable` | no | the saved password cannot be decrypted (`APP_KEY` changed) |
| `mail_invalid` | no | an address or header is invalid |
| `mail_template` | no | a template is missing, invalid or failed |
| `smtp_auth` | no | the server refused the credentials |
| `smtp_rejected` | no | the server refused the message (5xx) |
| `smtp_connect` | yes | no connection |
| `smtp_tls` | yes | TLS failed: no STARTTLS offered, a certificate not trusted or for another name |
| `smtp_temporary` | yes | the server deferred the message (4xx) |
| `smtp_timeout` | yes | too slow |
| `smtp_throttled` | yes | this process sent `max_per_minute` already |
