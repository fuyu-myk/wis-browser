//! The time range a clear-data request reaches back over, checked once for
//! both the history and the site-data commands.

use chrono::{DateTime, Utc};

use crate::error::{WisError, WisResult};

/// Turn the dialog's `since_unix` (seconds) into the instant a clear reaches
/// back to. `None` means all time. Refuses an instant before 1970 or after
/// `now`.
pub fn clear_since(
    since_unix: Option<i64>,
    now: DateTime<Utc>,
) -> WisResult<Option<DateTime<Utc>>> {
    let Some(secs) = since_unix else {
        return Ok(None);
    };
    if secs < 0 {
        return Err(WisError::InvalidInput {
            message: "the time range cannot start before 1970".to_string(),
        });
    }
    if secs > now.timestamp() {
        return Err(WisError::InvalidInput {
            message: "the time range cannot start in the future".to_string(),
        });
    }

    DateTime::from_timestamp(secs, 0)
        .map(Some)
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("{secs} is not a unix time"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).expect("a fixed instant")
    }

    #[test]
    fn none_means_all_time() {
        assert_eq!(clear_since(None, now()).unwrap(), None);
    }

    #[test]
    fn a_past_instant_round_trips() {
        let hour_ago = now() - Duration::hours(1);
        assert_eq!(
            clear_since(Some(hour_ago.timestamp()), now()).unwrap(),
            Some(hour_ago)
        );
        assert_eq!(
            clear_since(Some(now().timestamp()), now()).unwrap(),
            Some(now())
        );
    }

    #[test]
    fn instants_outside_the_offered_ranges_are_refused() {
        assert!(clear_since(Some(-1), now()).is_err());
        assert!(clear_since(Some(now().timestamp() + 1), now()).is_err());
    }
}
