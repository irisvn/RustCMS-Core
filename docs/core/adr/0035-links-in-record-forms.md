# ADR 0035: Belongs-to-many links in the record's own form

- Status: accepted (0.9.0, C1)
- Date: 2026-10-24

## Context

ADR 0020 deferred choosing belongs-to-many links in the record's create and
edit form ("replacing a set, stale checks, hidden links"): links were made
only in the relation manager on the record's page. Applications want the
usual multi-select (tags on a post) in the form, without a second path that
authorizes differently. Three risks shaped the decision:

- **Replacing a set loses data.** A form that sends "the links are now
  {a, b}" undoes links another user made meanwhile, and links the user cannot
  see (the target's scope or view policy) would look absent and be removed.
- **Two write paths drift apart.** The relation manager already admits
  `attach_{rel}` / `detach_{rel}` before reading anything, checks the parent
  per record, locks the targets the user may view and re-checks them, keeps
  one lock order, audits on the parent and (since C1) calls
  `Resource::after_relation_change`.
- **Records do not carry links.** Envelopes hold columns only; adding the
  links of every relation to every record read would cost queries on every
  list.

## Decision

- **Declared as an input.** `BelongsToManySelect::make("tags")` in
  `Resource::form()` names a `Relation::belongs_to_many` of the resource
  (`InputType::BelongsToMany`, `belongs_to_many` on the wire, vocabulary 9).
  Boot refuses a name that is not such a relation, that is also a field,
  that appears twice, a relation to the media library (writes hold media
  rows under shared locks for the record's references first; locking them
  again as link targets would upgrade those locks; media lists are
  `Field::media_list` fields), or a relation with required pivot fields (their values
  are asked for by the relation manager's attach form; the form input gives
  none, so new links take the pivot columns' database defaults). The
  automatic form never adds one.
- **The value is a change, not a set.** The form sends
  `{"tags": {"attach": [keys], "detach": [keys]}}` (each optional, at most
  100 canonical keys, never both for one key). Links not named are left
  alone, so a save never undoes another user's links or links the user cannot
  see; attaching a linked target or detaching an unlinked one changes
  nothing (as in the relation manager). Detaching on create is a 422.
- **The relation manager's rules, on the record's write.** In `create` and
  `update`, after the record's own admission: `attach_{rel}` / `detach_{rel}`
  are admitted (`Policy::admits`, 403) for the parts sent non-empty, before
  their keys are read; then the shape (422 on the input's name). In the
  write's transaction, once the record's references are checked and before
  its row is written (its foreign keys take shared locks on what they
  reference; locking the same rows exclusively afterwards would upgrade them
  and could deadlock two such writes), the targets named are locked through
  `lock_visible` (scope and view policy,
  re-checked under the lock, key order; the targets of every relation sent
  that share a target resource are locked together, resources in the
  registry's lock order (`Registry::lock_rank`: parents before their
  belongs-to-many targets and `lock_parent` children, resources the lock
  graph does not order by slug), so two relations to one resource never lock
  its rows in different orders). When the row's own `belongs_to` keys
  reference rows of a target resource, those rows are locked in the same
  pass (`DynResource::lock_visible_with`: one key-order pass, references
  locked whoever may view them, hidden targets never locked): otherwise the
  row's write would take their shared locks after the targets' exclusive
  ones, and two writes crossing (a post's primary tag one way, its tags the
  other) could deadlock. References into other resources keep their shared
  locks: locking them exclusively here would invert the order of writers
  that lock them first (a category's delete setting its posts' key to null).
  After the row is written (and its parent lock checked), the policy decides
  `attach_{rel}` / `detach_{rel}` on the record as stored by this write (403,
  the whole write rolls back), and a target to attach that is not
  visible is a 422 (`is not one of the options`), one to detach is ignored.
  Then the pivot rows, `after_relation_change` once per relation with the
  targets that changed, `after_save`, the record's audit entry, then the
  `attached` / `detached` entries. Updates also need `update` on the record,
  as for any field. One lock order is kept: parent ancestors, the record
  (updates), the targets, then the record's row and the pivot rows.
- **Reads go through existing, authorized endpoints.** The edit form reads
  the current links from the relation's list
  (`GET …/records/{key}/relations/{rel}?per_page=100`: target scope and
  policy); with more than 100 the input is read-only and points to the
  record's page. Targets to attach come from
  `GET …/records/{key}/relations/{rel}/options` on edit, and from a new
  `GET {admin}/api/resources/{slug}/relations/{relation}/options` on create
  (admitted on `attach_{rel}`, then `create` and `attach_{rel}` allowed
  without a record; the target's lookup decides which records appear; oracle
  cases in `fixtures/authz/admin.toml`).
- **The schema says what the user may change.** `InputSchema.links =
  {attach, detach}`: on create, `attach` when admitted and allowed without a
  record; on edit, both decided on the record; the input is left out when
  neither holds, when the user may not list the target, and on view schemas
  (the relation manager shows the links there).

## Consequences

- Removing a link from the form needs `detach_{rel}`; the SPA only offers
  removing current links with it, and records chosen in the same form can
  always be put back before saving.
- Pivot values cannot be set from the form; relations with required pivot
  fields keep the relation manager only.
- `InputType`, `InputSchema` (`links`) and `DynResource`
  (`link_options_new`) gain members: exhaustive matches and struct literals
  in applications need them. Older SPAs see vocabulary 9: they show these inputs read-only and never send them.
- No idempotency beyond the record's: a create's `Idempotency-Key` claim
  covers its links (the claim hashes the whole input).
