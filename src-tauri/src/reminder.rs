#![cfg_attr(not(feature = "desktop"), allow(dead_code))]

use crate::models::{Settings, UsageSnapshot};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderCandidate {
    pub key: String,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Default)]
pub struct ReminderState {
    delivered_keys: HashSet<String>,
}

impl ReminderState {
    pub fn take_new(&mut self, candidates: Vec<ReminderCandidate>) -> Vec<ReminderCandidate> {
        candidates
            .into_iter()
            .filter(|candidate| self.delivered_keys.insert(candidate.key.clone()))
            .collect()
    }
}

pub fn evaluate_reminders(
    snapshot: Option<&UsageSnapshot>,
    settings: &Settings,
    now_seconds: u64,
) -> Vec<ReminderCandidate> {
    if !settings.notifications_enabled {
        return Vec::new();
    }

    let Some(snapshot) = snapshot else {
        return Vec::new();
    };

    let mut reminders = Vec::new();
    if let Some(percent) = snapshot.remaining_percent {
        if let Some(threshold) = matched_threshold(percent, &settings.notify_below_percent) {
            reminders.push(ReminderCandidate {
                key: format!("low:{}:{threshold}", snapshot.id),
                title: "QuotaDock usage is low".to_string(),
                body: format!(
                    "Codex remaining usage is {percent}%, below the {threshold}% threshold."
                ),
            });
        }
    }

    match parsed_at_age_minutes(&snapshot.parsed_at, now_seconds) {
        Some(age) if age > settings.stale_after_minutes as u64 => {
            reminders.push(ReminderCandidate {
                key: format!("stale:{}:{}", snapshot.id, settings.stale_after_minutes),
                title: "QuotaDock snapshot is stale".to_string(),
                body: format!("Latest Codex usage snapshot is {age} minutes old."),
            });
        }
        None => {
            reminders.push(ReminderCandidate {
                key: format!("stale-unknown:{}", snapshot.id),
                title: "QuotaDock snapshot age is unknown".to_string(),
                body: "Latest Codex usage snapshot has an unrecognized timestamp.".to_string(),
            });
        }
        _ => {}
    }

    reminders
}

pub fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn matched_threshold(percent: u8, thresholds: &[u8]) -> Option<u8> {
    thresholds
        .iter()
        .copied()
        .filter(|threshold| percent <= *threshold)
        .min()
}

fn parsed_at_age_minutes(parsed_at: &str, now_seconds: u64) -> Option<u64> {
    let parsed_seconds = parsed_at.strip_prefix("unix:")?.parse::<u64>().ok()?;
    Some(now_seconds.saturating_sub(parsed_seconds) / 60)
}

#[cfg(feature = "desktop")]
pub fn notify_due_reminders(app: &tauri::AppHandle, state: &crate::models::AppState) {
    use tauri::Manager;
    use tauri_plugin_notification::NotificationExt;

    let candidates = evaluate_reminders(
        state.latest_snapshot.as_ref(),
        &state.settings,
        now_seconds(),
    );
    let reminders = {
        let reminder_state = app.state::<std::sync::Mutex<ReminderState>>();
        let mut reminder_state = match reminder_state.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        reminder_state.take_new(candidates)
    };

    for reminder in reminders {
        let _ = app
            .notification()
            .builder()
            .title(reminder.title)
            .body(reminder.body)
            .show();
    }
}

#[cfg(all(not(test), not(feature = "desktop")))]
pub fn notify_due_reminders(_app: &tauri::AppHandle, _state: &crate::models::AppState) {}

#[cfg(test)]
mod tests {
    use crate::models::{ConfidenceState, Settings, SnapshotSource, UsageSnapshot};
    use crate::reminder::{evaluate_reminders, ReminderState};

    fn settings() -> Settings {
        Settings {
            stale_after_minutes: 60,
            notify_below_percent: vec![20, 10],
            clipboard_monitoring: false,
            notifications_enabled: true,
        }
    }

    fn snapshot(percent: Option<u8>, parsed_at: &str) -> UsageSnapshot {
        UsageSnapshot {
            id: "snapshot-1".to_string(),
            source: SnapshotSource::PastedStatus,
            parsed_at: parsed_at.to_string(),
            remaining_percent: percent,
            reset_at: None,
            reset_countdown_seconds: None,
            credits_balance: None,
            model: None,
            context_window: None,
            confidence: ConfidenceState::Fresh,
            raw_text: String::new(),
            manual_fields: Vec::new(),
            warnings: Vec::new(),
            notes: String::new(),
        }
    }

    #[test]
    fn emits_low_balance_for_nearest_threshold() {
        let reminders =
            evaluate_reminders(Some(&snapshot(Some(8), "unix:1000")), &settings(), 1020);

        assert_eq!(reminders.len(), 1);
        assert!(reminders[0].key.ends_with(":10"));
        assert!(reminders[0].body.contains("8%"));
    }

    #[test]
    fn emits_stale_snapshot_reminder() {
        let reminders =
            evaluate_reminders(Some(&snapshot(Some(80), "unix:1000")), &settings(), 5_000);

        assert_eq!(reminders.len(), 1);
        assert!(reminders[0].key.starts_with("stale:"));
    }

    #[test]
    fn notifications_can_be_disabled() {
        let mut settings = settings();
        settings.notifications_enabled = false;

        let reminders = evaluate_reminders(Some(&snapshot(Some(1), "unix:1000")), &settings, 5_000);

        assert!(reminders.is_empty());
    }

    #[test]
    fn reminder_state_deduplicates_keys() {
        let mut state = ReminderState::default();
        let reminders =
            evaluate_reminders(Some(&snapshot(Some(9), "unix:1000")), &settings(), 1020);

        assert_eq!(state.take_new(reminders.clone()).len(), 1);
        assert!(state.take_new(reminders).is_empty());
    }
}
