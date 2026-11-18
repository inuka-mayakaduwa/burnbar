//! Conservative, local-only status and pace information
//! (Go: `internal/insights/`).

use crate::history::Sample;
use crate::usage::Usage;
use chrono::{DateTime, Timelike, Utc};
use std::time::Duration;

pub fn age(at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(at) = at else {
        return "Never updated".into();
    };
    let duration = (now - at).max(chrono::Duration::zero());
    if duration < chrono::Duration::minutes(1) {
        return "Updated just now".into();
    }
    if duration < chrono::Duration::hours(1) {
        return format!("Updated {} min ago", duration.num_minutes());
    }
    if duration < chrono::Duration::hours(24) {
        return format!("Updated {}h ago", duration.num_hours());
    }
    format!("Updated {}d ago", duration.num_hours() / 24)
}

/// Freshness refers to actual quota data, never the timestamp of a failed attempt.
pub fn freshness(usage: &Usage, now: DateTime<Utc>) -> String {
    if usage.is_empty() || usage.updated_at.is_none() {
        return "No successful reading yet".into();
    }
    age(usage.updated_at, now)
}

pub fn health(usage: &Usage, now: DateTime<Utc>, stale_after: Duration) -> String {
    if !usage.err.is_empty() {
        let err = usage.err.to_lowercase();
        if err.contains("loading") {
            return "Loading".into();
        }
        if usage.rate_limited {
            if let Some(retry_at) = usage.retry_at {
                if retry_at > now {
                    let seconds = (retry_at - now).num_seconds().max(0);
                    if seconds >= 60 {
                        return format!(
                            "Rate limited — retry in {}m {:02}s",
                            seconds / 60,
                            seconds % 60
                        );
                    }
                    return format!("Rate limited — retry in {}s", seconds);
                }
            }
            return "Rate limited — waiting to retry".into();
        }
        if err.contains("login")
            || err.contains("sign in")
            || err.contains("token")
            || err.contains("credentials")
        {
            return "Sign-in required".into();
        }
        if err.contains("network")
            || err.contains("timeout")
            || err.contains("timed out")
            || err.contains("connection")
            || err.contains("dial ")
        {
            return "Offline / connection error".into();
        }
        return "Usage unavailable".into();
    }
    if usage.is_empty() || usage.updated_at.is_none() {
        return "Waiting for usage".into();
    }
    let age = now - usage.updated_at.unwrap_or(now);
    if age > chrono::Duration::from_std(stale_after).unwrap_or(chrono::Duration::hours(1)) {
        return "Stale reading".into();
    }
    String::new()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Forecast {
    pub remaining: Duration,
    pub before_reset: bool,
}

impl Forecast {
    pub fn text(&self) -> String {
        if !self.before_reset {
            return "Recent pace: reset expected before limit".into();
        }
        let minutes = ((self.remaining.as_secs_f64() / 60.0) / 5.0)
            .round()
            .max(1.0) as u64
            * 5;
        let duration = if minutes >= 60 {
            format!("{}h {:02}m", minutes / 60, minutes % 60)
        } else {
            format!("{} min", minutes)
        };
        format!("At recent pace: ~{duration} until limit")
    }
}

/// Requires at least three contiguous observations covering 20 minutes,
/// growth of 1 percentage point, and a known current quota cycle. Reset or
/// correction drops and long gaps cut off history; stale or failed readings
/// never get forecasts.
pub fn estimate(
    usage: &Usage,
    samples: &[Sample],
    secondary: bool,
    now: DateTime<Utc>,
) -> Option<Forecast> {
    let window = if secondary {
        usage.seven_day.as_ref()?
    } else {
        usage.five_hour.as_ref()?
    };
    let updated_at = usage.updated_at?;
    let resets_at = window.resets_at?;
    if !usage.err.is_empty()
        || usage.rate_limited
        || window.length_secs <= 0
        || resets_at <= now
        || now - updated_at > chrono::Duration::minutes(10)
        || updated_at > now + chrono::Duration::minutes(1)
        || !(0.0..100.0).contains(&window.percent)
    {
        return None;
    }
    let length = chrono::Duration::seconds(window.length_secs);
    let start = resets_at - length;
    let cutoff = start.max(now - chrono::Duration::hours(3));
    let anchor = updated_at.with_nanosecond(0).unwrap_or(updated_at);
    let mut last_t = anchor;
    let mut last_p = window.percent;
    let mut first_t = last_t;
    let mut first_p = last_p;
    let mut count = 1usize;

    for sample in samples.iter().rev() {
        let t = DateTime::from_timestamp(sample.t, 0)?;
        let p = if secondary { sample.w } else { sample.s };
        if t >= last_t {
            continue;
        }
        if t < cutoff {
            break;
        }
        if p < 0.0
            || p > 100.0
            || p.is_nan()
            || last_p < p
            || last_t - t > chrono::Duration::minutes(20)
        {
            break;
        }
        first_t = t;
        first_p = p;
        last_t = t;
        last_p = p;
        count += 1;
    }

    let span = anchor - first_t;
    let growth = window.percent - first_p;
    if count < 3 || span < chrono::Duration::minutes(20) || growth < 1.0 {
        return None;
    }
    let seconds = (100.0 - window.percent) / growth * span.num_milliseconds() as f64 / 1000.0;
    if seconds <= 0.0 || !seconds.is_finite() {
        return None;
    }
    let remaining = Duration::from_secs_f64(seconds);
    let eta = now + chrono::Duration::from_std(remaining).ok()?;
    Some(Forecast {
        remaining,
        before_reset: eta < resets_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{Provider, Window};

    fn usage(percent: f64, now: DateTime<Utc>) -> Usage {
        let mut u = Usage::new(Provider::Claude);
        u.updated_at = Some(now);
        u.five_hour = Some(
            Window::new("session", "Session", percent)
                .with_reset(Some(now + chrono::Duration::hours(3)))
                .with_length(Duration::from_secs(5 * 3600)),
        );
        u
    }

    fn sample(at: DateTime<Utc>, percent: f64) -> Sample {
        Sample {
            t: at.timestamp(),
            s: percent,
            w: percent,
        }
    }

    #[test]
    fn age_and_health_strings() {
        let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        assert_eq!(age(None, now), "Never updated");
        assert_eq!(
            age(Some(now - chrono::Duration::seconds(30)), now),
            "Updated just now"
        );
        assert_eq!(
            age(Some(now - chrono::Duration::minutes(5)), now),
            "Updated 5 min ago"
        );
        assert_eq!(
            age(Some(now - chrono::Duration::hours(3)), now),
            "Updated 3h ago"
        );
        assert_eq!(
            age(Some(now - chrono::Duration::days(2)), now),
            "Updated 2d ago"
        );

        let mut u = usage(50.0, now);
        assert_eq!(health(&u, now, Duration::from_secs(600)), "");
        assert_eq!(freshness(&u, now), "Updated just now");
        u.err = "network error: offline".into();
        assert_eq!(
            health(&u, now, Duration::from_secs(600)),
            "Offline / connection error"
        );
        u.rate_limited = true;
        u.retry_at = Some(now + chrono::Duration::seconds(125));
        assert_eq!(
            health(&u, now, Duration::from_secs(600)),
            "Rate limited — retry in 2m 05s"
        );
    }

    #[test]
    fn forecast_needs_three_contiguous_growing_samples() {
        let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let u = usage(40.0, now);
        let samples = vec![
            sample(now - chrono::Duration::minutes(40), 20.0),
            sample(now - chrono::Duration::minutes(20), 30.0),
        ];
        let forecast = estimate(&u, &samples, false, now).unwrap();
        // 20 points over 40 minutes => 60 points left => 120 minutes.
        assert!((forecast.remaining.as_secs_f64() - 7_200.0).abs() < 5.0);
        assert!(forecast.before_reset);
        assert!(forecast.text().contains("2h 00m"));
        assert!(estimate(&u, &samples[..1], false, now).is_none());
    }

    #[test]
    fn forecast_stops_at_a_drop_or_gap() {
        let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let u = usage(40.0, now);
        let samples = vec![
            sample(now - chrono::Duration::minutes(35), 30.0),
            sample(now - chrono::Duration::minutes(20), 10.0), // correction drop
        ];
        assert!(estimate(&u, &samples, false, now).is_none());
        let samples = vec![
            sample(now - chrono::Duration::minutes(120), 30.0),
            sample(now - chrono::Duration::minutes(80), 35.0), // >20 minute gap
        ];
        assert!(estimate(&u, &samples, false, now).is_none());
    }
}
