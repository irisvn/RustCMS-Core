# What is kept, and for how long

`serve` deletes expired data on its own, once per period. With Redis, one
instance at a time does it; without, every instance runs the pass (each
deletion is safe to repeat). The commands do the same on demand.

| Data | Kept | Pruned by |
|---|---|---|
| Sessions | until idle `session_lifetime_minutes` (remembered: `remember_days`) | `serve` hourly, `session:prune` |
| JWT sign-ins | until their family ends (`refresh_family_days`) | `serve` hourly |
| Reset and invitation links | until they expire (marks of credential changes: 30 days) | `serve` hourly |
| Audit entries | `audit.retention_days` (365) | `serve` hourly, `audit:prune` |
| Idempotency receipts | 24 hours | `serve`, `idempotency:prune` |
| Failed jobs, `once` receipts | `queue.failed_retention_days` (30) | the workers |
| Background exports (file and row) | 24 hours, removed an hour later | `serve` |
| Imports (file, row, report, receipts) | 24 hours from upload, or from the start of the run | `serve` |
| Media files of deleted media records, interrupted uploads | removed within minutes (queued deletions), an hour (abandoned uploads) | `serve` every minute, `media:prune` |
| Uploaded media | until deleted from the Media library (a cover removed from a record, or an abandoned draft's, stays) | by hand |

Run the commands from cron when `serve` is not running (workers only, or
maintenance windows):

```sh
rustcms session:prune
rustcms audit:prune
rustcms idempotency:prune
rustcms media:prune
```

Queued jobs hold their payload until they are done or, failed, until pruned:
a password reset job holds the address it was asked for (never a link). Keep
backups of the database as protected as the database itself.
