//! Charge every HTTP attempt, including probes, fallbacks and malformed replies.
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
#[derive(Default)]
pub(super) struct Quota {
    requests: VecDeque<Instant>,
    tokens: u64,
}
impl Quota {
    pub fn reserve(
        &mut self,
        at: Instant,
        rpm: usize,
        cap: u64,
        reserve: u64,
    ) -> Result<(), &'static str> {
        self.requests
            .retain(|t| at.duration_since(*t) < Duration::from_secs(60));
        if self.requests.len() >= rpm {
            return Err("AI request rate budget reached");
        }
        if self.tokens.saturating_add(reserve) > cap {
            return Err("AI session token budget reached");
        }
        self.tokens = self.tokens.saturating_add(reserve);
        self.requests.push_back(at);
        Ok(())
    }
    pub fn settle(&mut self, reserved: u64, total: Option<u64>) {
        if let Some(actual) = total {
            self.tokens = self.tokens.saturating_sub(reserved).saturating_add(actual);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retries_share_budget_and_expiry_uses_monotonic_time() {
        let mut q = Quota::default();
        let t = Instant::now();
        q.reserve(t, 1, 50, 35).unwrap();
        q.settle(35, Some(20));
        assert!(q.reserve(t, 1, 50, 10).is_err());
        let later = t + Duration::from_secs(60);
        assert!(q.reserve(later, 1, 50, 31).is_err());
        q.reserve(later, 1, 50, 30).unwrap();
    }
}
