//! Locora Cloud as the UI sees it: the runtime's sync reports and summaries as `locora-app-api`
//! types, the same for the desktop and the browser.

use locora_app_api::{
    CloudAutoOutcome, CloudAutoState, CloudChangeKind, CloudChanges, CloudPalaceChange,
    CloudStatus, CloudSyncResult,
};
use locora_runtime::cloud::{self, CloudState};
use locora_runtime::cloud_http;
use locora_runtime::cloud_sync::{SyncHold, SyncReport, hold_of};
use locora_store::{ChangeKind, ChangeSummary, PalaceChange, Store};

pub fn status(state: Option<&CloudState>) -> CloudStatus {
    match state {
        Some(state) => CloudStatus {
            account: Some(state.account.email.clone()),
            server: cloud::check_server(&state.server).ok(),
            access: cloud::can_sync(state, cloud::now_secs()),
            email_verified: state.account.email_verified,
            auto_sync: state.auto_sync,
            last_sync_at: state.last_sync_at,
        },
        None => CloudStatus::signed_out(),
    }
}

pub fn palace(change: &PalaceChange) -> CloudPalaceChange {
    CloudPalaceChange {
        id: change.id.clone(),
        name: change.name.clone(),
        kind: match change.kind {
            ChangeKind::New => CloudChangeKind::New,
            ChangeKind::Changed => CloudChangeKind::Changed,
            ChangeKind::Deleted => CloudChangeKind::Deleted,
        },
        changes: change.changes,
    }
}

pub fn changes(summary: &ChangeSummary) -> CloudChanges {
    CloudChanges {
        total: summary.total,
        palaces: summary.palaces.iter().map(palace).collect(),
        other: summary.other,
    }
}

pub fn sync_result(report: &SyncReport) -> CloudSyncResult {
    CloudSyncResult {
        pushed: report.pushed,
        media_uploaded: report.media_uploaded,
        pulled: report.pulled,
        media_downloaded: report.media_downloaded,
        applied: report.applied.applied,
        stale: report.applied.stale,
        missing: report.applied.missing,
        failed: report.applied.failed.len(),
        quarantined: report.quarantined.len(),
        cloud_expires_at: report.cloud_expires_at,
        received: changes(&report.received),
        compacted: report.compacted,
        too_large: report.too_large,
    }
}

/// The outbox of this device for the auto-sync driver.
pub fn auto_state(state: Option<&CloudState>, store: &Store) -> anyhow::Result<CloudAutoState> {
    let local_seq = store.last_op_seq()?;
    Ok(CloudAutoState {
        enabled: state.is_some_and(|state| state.auto_sync),
        local_seq,
        pending: local_seq > store.sync_cursor("push")?,
        last_sync_at: state.and_then(|state| state.last_sync_at),
    })
}

/// An automatic sync's result for the UI: a finished sync, palace deletions to confirm, a pause
/// that needs a person, or an error to retry later.
pub fn auto_outcome(result: anyhow::Result<SyncReport>) -> CloudAutoOutcome {
    let mut outcome = CloudAutoOutcome {
        enabled: true,
        ..CloudAutoOutcome::default()
    };
    match result {
        Ok(report) => outcome.result = Some(sync_result(&report)),
        Err(error) => match hold_of(&error) {
            Some(SyncHold::Confirm(palaces)) => {
                outcome.confirm = palaces.iter().map(palace).collect();
            }
            Some(SyncHold::Rebuild) => outcome.paused = Some(error.to_string()),
            None => match cloud_http::status_of(&error) {
                // The session ended (signed out elsewhere, password changed) or the access
                // ended: a person has to act.
                Some(401..=403) => outcome.paused = Some(format!("{error:#}")),
                // Too many requests or a busy server: waits longer before the next try.
                Some(429 | 503) => {
                    outcome.rate_limited = true;
                    outcome.error = Some(format!("{error:#}"));
                }
                _ => outcome.error = Some(format!("{error:#}")),
            },
        },
    }
    outcome
}

/// The runtime's answer about the rankings, as the UI reads it.
pub fn rankings(
    outcome: locora_runtime::cloud::RankingsDeletion,
) -> locora_app_api::RankingsDeletion {
    use locora_app_api::RankingsDeletion as Ui;
    use locora_runtime::cloud::RankingsDeletion as Run;
    match outcome {
        Run::NotSignedIn => Ui::NotSignedIn,
        Run::Deleted => Ui::Deleted,
        Run::ServerTooOld => Ui::ServerTooOld,
        Run::Pending => Ui::Pending,
        Run::SignInAgain => Ui::SignInAgain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hold_and_a_refusal_are_told_apart_from_an_error() {
        let confirm = auto_outcome(Err(SyncHold::Confirm(vec![PalaceChange {
            id: "p".into(),
            name: Some("Haus".into()),
            kind: ChangeKind::Deleted,
            changes: 1,
        }])
        .into()));
        assert_eq!(confirm.confirm.len(), 1);
        assert_eq!(confirm.confirm[0].kind, CloudChangeKind::Deleted);
        assert!(confirm.error.is_none() && confirm.result.is_none());
        let rebuild = auto_outcome(Err(SyncHold::Rebuild.into()));
        assert!(rebuild.paused.is_some());
        let busy = auto_outcome(Err(cloud_http::failure(&cloud_http::Reply {
            status: 429,
            body: Vec::new(),
        })));
        assert!(busy.rate_limited && busy.error.is_some());
        let crowded = auto_outcome(Err(cloud_http::failure(&cloud_http::Reply {
            status: 503,
            body: Vec::new(),
        })));
        assert!(crowded.rate_limited && crowded.error.is_some());
        let gone = auto_outcome(Err(cloud_http::failure(&cloud_http::Reply {
            status: 401,
            body: br#"{"error":"Anmeldung abgelaufen"}"#.to_vec(),
        })));
        assert_eq!(gone.paused.as_deref(), Some("Anmeldung abgelaufen"));
    }
}
