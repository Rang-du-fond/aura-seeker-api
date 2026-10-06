use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::Duration,
};

use crate::{Error, Result};

const UNKNOWN_CLIENT: &str = "unknown";

pub struct Limiter {
    limit: usize,
    window: u64,
    hits: Mutex<HashMap<String, VecDeque<u64>>>,
}

impl Limiter {
    pub fn new(limit: usize, window: Duration) -> Self {
        Self { limit, window: window.as_secs(), hits: Mutex::default() }
    }

    pub fn admit(&self, client: Option<&str>, now: u64) -> Result<()> {
        let mut hits = self.hits.lock().map_err(|_| Error::unexpected("the rate limiter lock is poisoned"))?;
        hits.retain(|_, times| still_counted(times, now.saturating_sub(self.window)));
        let times = hits.entry(client.unwrap_or(UNKNOWN_CLIENT).to_owned()).or_default();
        let admitted = times.len() < self.limit;
        if admitted {
            times.push_back(now);
        }
        drop(hits);
        admitted.then_some(()).ok_or(Error::RateLimited)
    }
}

fn still_counted(times: &mut VecDeque<u64>, window_start: u64) -> bool {
    while times.front().is_some_and(|time| *time <= window_start) {
        times.pop_front();
    }
    !times.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_is_refused_beyond_the_limit_until_the_window_passes() {
        let limiter = Limiter::new(2, Duration::from_secs(60));
        assert!(limiter.admit(Some("1.2.3.4"), 100).is_ok());
        assert!(limiter.admit(Some("1.2.3.4"), 110).is_ok());
        assert!(matches!(limiter.admit(Some("1.2.3.4"), 120), Err(Error::RateLimited)));
        assert!(limiter.admit(Some("5.6.7.8"), 120).is_ok());
        assert!(limiter.admit(Some("1.2.3.4"), 161).is_ok());
    }
}
