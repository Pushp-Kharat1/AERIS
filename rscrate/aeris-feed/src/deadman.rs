use aeris_core::{TimestampNs, Venue};
use std::collections::HashMap;

pub struct DeadManSnitch {
    timeout_ns: u64,
    last_heartbeat_ns: HashMap<Venue, TimestampNs>,
}

impl DeadManSnitch {
    pub fn new(timeout_ms: u64) -> Self {
        Self {
            timeout_ns: timeout_ms * 1_000_000,
            last_heartbeat_ns: HashMap::new(),
        }
    }

    pub fn record_heartbeat(&mut self, venue: Venue, timestamp_ns: TimestampNs) {
        self.last_heartbeat_ns.insert(venue, timestamp_ns);
    }

    pub fn check_stalled_venues(&self, current_time_ns: TimestampNs) -> Vec<Venue> {
        let mut stalled = Vec::new();
        for (&venue, &last_ts) in &self.last_heartbeat_ns {
            if current_time_ns.saturating_sub(last_ts) >= self.timeout_ns {
                stalled.push(venue);
            }
        }
        stalled
    }
}
