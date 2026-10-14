//! Audit entries of authentication events (ADR 0014). They are not data
//! changes: recorded on their own, best effort (a failure is logged).

use chrono::Utc;
use rustcms_core::AppState;
use rustcms_core::audit::{AuditActor, AuditConfig, AuditContext, AuditEntry, RequestMeta};
use serde::Serialize;

use crate::state::AuthState;

/// An entry to record once the session is stored (sign-in, sign-out): the
/// guard puts it in the log after persisting, never for a session that failed
/// to save.
#[derive(Debug, Clone)]
pub struct AfterSession(pub AuditContext, pub AuditEntry);

/// What a failed sign-in records: the address tried, whatever the reason.
#[derive(Serialize)]
struct FailedSignIn<'a> {
    email: String,
    guard: &'a str,
}

/// Failed sign-ins not recorded one by one in a minute.
#[derive(Serialize)]
struct FailedBurst {
    count: u64,
}

/// Records a failed sign-in as a guest, the same way for every reason. Beyond
/// `audit.sign_in_failures_per_minute` (per instance), failures are counted
/// and recorded as one `auth.sign_in_failed_burst` entry for the minute.
pub(crate) async fn sign_in_failed(app: &AppState, auth: &AuthState, request: &RequestMeta, email: &str, guard: &str) {
    let limit = AuditConfig::of(app.config()).map_or(600, |c| c.sign_in_failures_per_minute) as u64;
    let minute = Utc::now().timestamp() / 60;
    let (record, burst) =
        admit(&mut auth.failed_sign_ins.lock().unwrap_or_else(std::sync::PoisonError::into_inner), minute, limit);
    let guest = AuditContext::new(AuditActor::guest(), request.clone());
    if burst > 0 {
        let entry = AuditEntry::new("auth.sign_in_failed_burst", "session").meta(FailedBurst { count: burst });
        rustcms_core::audit::record_detached(
            app.db(),
            &AuditContext::new(AuditActor::guest(), RequestMeta::default()),
            entry,
        )
        .await;
    }
    if record {
        let email: String = email.chars().take(255).collect();
        let entry = AuditEntry::new("auth.sign_in_failed", "session").meta(FailedSignIn { email, guard });
        rustcms_core::audit::record_detached(app.db(), &guest, entry).await;
    }
}

/// Counts a failure in `window` (`(minute, recorded, skipped)`): whether to
/// record it, and how many failures a previous minute skipped (to summarize).
fn admit(window: &mut (i64, u64, u64), minute: i64, limit: u64) -> (bool, u64) {
    let burst = if window.0 == minute {
        0
    } else {
        let skipped = window.2;
        *window = (minute, 0, 0);
        skipped
    };
    let record = window.1 < limit;
    if record {
        window.1 += 1;
    } else {
        window.2 += 1;
    }
    (record, burst)
}

#[cfg(test)]
mod tests {
    use super::admit;

    #[test]
    fn failures_beyond_the_cap_are_summarized_the_next_minute() {
        let mut window = (0, 0, 0);
        let results: Vec<_> = (0..5).map(|_| admit(&mut window, 7, 2)).collect();
        assert_eq!(results, [(true, 0), (true, 0), (false, 0), (false, 0), (false, 0)]);
        // The next failure, a later minute: recorded, with the 3 skipped before.
        assert_eq!(admit(&mut window, 9, 2), (true, 3));
        assert_eq!(admit(&mut window, 9, 2), (true, 0));
        // A minute with nothing skipped summarizes nothing.
        assert_eq!(admit(&mut window, 10, 2), (true, 0));
    }
}
