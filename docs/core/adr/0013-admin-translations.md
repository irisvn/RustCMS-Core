# 0013 — Translations of the admin interface

- Status: accepted
- Date: 2026-10-16

## Context

0.3 targets internal back-offices used in Vietnamese and English. Only the
interface is translated; managed content keeps one language. Text reaches the
user from three places: the panel's own code, labels the application declares
(sent by the server in schemas), and server validation messages, some of them
written by application hooks.

## Decision

- **One language per request.** The language the panel shows
  (`X-Rustcms-Locale`), so a tab's cache always matches its labels; the panel
  takes it from `/me`: the user's saved choice (`users.locale`) when still
  offered, else `Accept-Language`, else `app.locale`. No language cookie: the account is
  the source, so every device agrees, and before sign-in the panel picks from
  the browser (and a choice kept on the device).
- **The panel's text** lives in typed catalogs (`admin-ui/src/i18n`); a local
  lint rule refuses text written in place.
- **Application labels** are translated by the server when it builds schemas
  and navigation, from TOML catalogs keyed by the English source string (like
  Laravel's `lang/vi.json`), optionally per resource, compiled into the binary.
  Source strings need no keys, labels formed from column names are covered,
  and `lang:missing` finds what is left. Placeholders are checked at boot.
- **Validation messages stay English on the wire**, with a stable code and
  parameters derived from one table of English templates (`issues`,
  `formIssues`); the panel translates codes. API clients keep the messages
  they had; no response is rewritten after the fact, and a message whose code
  the panel does not know is shown in English. Application messages without a
  template are `custom`; hooks translate them with `ctx.t`. A test scans the
  framework's sources so no message lacks a code.
- **Switching language** keeps forms and drafts: language-dependent queries
  are keyed by language and show the previous answer while loading; a form
  relabels its fixed structure. The choice is saved before the panel switches,
  so nothing fetched for the new language comes back in the old one.
- Uncertain writes are flagged by the client, never inferred from message text.

## Consequences

- One more column on `users`; one more admin route (`POST /auth/locale`).
- Adding a language means a panel catalog (`admin-ui/src/i18n/<locale>.ts`),
  the framework's server catalog, and `app.locales`.
- Messages built by hooks at run time are only translated if the hook uses
  `ctx.t`; `lang:missing` cannot see them.
