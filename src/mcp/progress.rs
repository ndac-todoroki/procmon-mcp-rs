//! `notifications/progress` for long tool calls, so clients keep waiting.

use serde_json::{Value, json};

/// Sends progress for one request, at most once per `interval_s` of reported time.
pub struct ProgressReporter<'a> {
    token: Option<Value>,
    notify: &'a mut dyn FnMut(Value),
    interval_s: f64,
    last_sent: Option<f64>,
}

impl<'a> ProgressReporter<'a> {
    /// `token` is the request's `_meta.progressToken`; without one, nothing is sent.
    pub fn new(token: Option<Value>, notify: &'a mut dyn FnMut(Value), interval_s: f64) -> Self {
        ProgressReporter {
            token,
            notify,
            interval_s,
            last_sent: None,
        }
    }

    pub fn report(&mut self, elapsed_s: f64, total_s: f64) {
        let Some(token) = &self.token else {
            return;
        };
        if self
            .last_sent
            .is_some_and(|last| elapsed_s - last < self.interval_s)
        {
            return;
        }
        self.last_sent = Some(elapsed_s);
        (self.notify)(json!({
            "jsonrpc": "2.0",
            "method": "notifications/progress",
            "params": {
                "progressToken": token,
                "progress": elapsed_s,
                "total": total_s,
                "message": "Capturing with Process Monitor"
            }
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sent(token: Option<Value>, reports: &[(f64, f64)]) -> Vec<Value> {
        let mut out = Vec::new();
        let mut notify = |v: Value| out.push(v);
        let mut reporter = ProgressReporter::new(token, &mut notify, 5.0);
        for &(elapsed, total) in reports {
            reporter.report(elapsed, total);
        }
        out
    }

    #[test]
    fn sends_nothing_without_a_progress_token() {
        assert!(sent(None, &[(0.0, 100.0), (10.0, 100.0)]).is_empty());
    }

    #[test]
    fn sends_progress_notifications_for_the_token() {
        let out = sent(Some(json!("tok-1")), &[(0.5, 100.0)]);
        assert_eq!(
            out,
            [json!({
                "jsonrpc": "2.0",
                "method": "notifications/progress",
                "params": {
                    "progressToken": "tok-1",
                    "progress": 0.5,
                    "total": 100.0,
                    "message": "Capturing with Process Monitor"
                }
            })]
        );
    }

    #[test]
    fn throttles_to_one_notification_per_interval() {
        let reports: Vec<_> = (0..=24).map(|i| (f64::from(i) * 0.5, 100.0)).collect();
        let progress: Vec<f64> = sent(Some(json!(7)), &reports)
            .iter()
            .map(|n| n["params"]["progress"].as_f64().unwrap())
            .collect();
        assert_eq!(progress, [0.0, 5.0, 10.0]);
    }
}
