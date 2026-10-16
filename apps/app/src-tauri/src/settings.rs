//! Settings and statistics screen.

use anyhow::{Result, bail};
use locora_api::review::ConfigureRequest;
use locora_app_api::{OptimizeDone, SettingsChange, SettingsView, StatsView};
use locora_runtime::Profile;
use locora_runtime::settings::{MAX_NEW_PER_DAY, parse_retention};

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

pub fn view(profile: &mut Profile) -> Result<SettingsView> {
    let review = if profile.has_review() {
        Some(profile.configure(ConfigureRequest::default())?)
    } else {
        None
    };
    let stats = profile.stats()?.map(|s| StatsView {
        answers: count(s.answers),
        answered_today: count(s.answered_today),
        new_today: count(s.new_today),
        recent: count(s.recent),
        recent_known: count(s.recent_known),
        streak: count(s.streak),
        scheduled: count(s.scheduled),
        due: count(s.due),
    });
    let (usable, hint) = profile.optimize_hint()?;
    Ok(SettingsView {
        usable_reviews: count(usable),
        needed_reviews: count(locora_runtime::settings::MIN_REVIEWS),
        optimize_hint: hint,
        new_per_day: count(profile.new_per_day()?),
        max_new_per_day: count(MAX_NEW_PER_DAY),
        retention: review.as_ref().map(|s| s.desired_retention),
        personal_weights: review.is_some_and(|s| s.weights.is_some()),
        stats,
    })
}

/// Fit personal FSRS weights from the review log and store them when they
/// predict better.
pub fn optimize(profile: &mut Profile) -> Result<OptimizeDone> {
    let result = profile.optimize(true)?;
    Ok(OptimizeDone {
        message: result.message(),
        view: view(profile)?,
    })
}

/// Apply a change. Every value is checked first; the plugin call (the one
/// that can still fail) runs before the daily count is written.
pub fn change(profile: &mut Profile, change: SettingsChange) -> Result<SettingsView> {
    let retention = change
        .retention
        .as_deref()
        .map(parse_retention)
        .transpose()?;
    if let Some(n) = change.new_per_day
        && n as usize > MAX_NEW_PER_DAY
    {
        bail!("Höchstens {MAX_NEW_PER_DAY} neue Loci pro Tag");
    }
    if let Some(v) = retention {
        profile.configure(ConfigureRequest {
            desired_retention: Some(v),
            ..Default::default()
        })?;
    }
    if let Some(n) = change.new_per_day {
        profile.set_new_per_day(n as usize)?;
    }
    view(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_reject_bad_values() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        let before = view(&mut profile).unwrap();
        assert_eq!(before.new_per_day, 10);
        assert_eq!((before.usable_reviews, before.optimize_hint), (0, false));
        let fitted = optimize(&mut profile).unwrap();
        assert!(
            fitted.message.starts_with("Noch zu wenig Daten: 0 von 100"),
            "{}",
            fitted.message
        );
        assert!(!fitted.view.personal_weights);
        assert!(before.stats.is_some(), "review plugin is built in");
        let after = change(
            &mut profile,
            SettingsChange {
                new_per_day: Some(15),
                retention: Some("85 %".into()),
            },
        )
        .unwrap();
        assert_eq!(after.new_per_day, 15);
        assert!((after.retention.unwrap() - 0.85).abs() < 1e-9);
        assert!(
            change(
                &mut profile,
                SettingsChange {
                    new_per_day: Some(5000),
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(
            change(
                &mut profile,
                SettingsChange {
                    retention: Some("viel".into()),
                    new_per_day: Some(3)
                }
            )
            .is_err()
        );
        for bad in ["0,5", "100 %"] {
            let attempt = SettingsChange {
                retention: Some(bad.into()),
                new_per_day: Some(4),
            };
            assert!(change(&mut profile, attempt).is_err(), "{bad}");
        }
        assert_eq!(
            view(&mut profile).unwrap().new_per_day,
            15,
            "nothing written on error"
        );
    }
}
