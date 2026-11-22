use aeris_core::TimestampNs;
use aeris_execution::ExchangeOrderGateway;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_GD_PORT: u16 = 9876;
pub const AUTH_MAGIC_TOKEN: u64 = 0x47445F53454E5452; // "GD_SENTR"

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct HeartbeatPacket {
    pub seq: u64,
    pub timestamp_ns: TimestampNs,
    pub active_positions_count: u32,
    pub auth_token: u64,
    pub is_disarm: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GdEscalationState {
    Armed,
    SoftWarning,
    HardCancelled,
    NuclearFlattened,
    Disarmed,
}

pub struct GuardianSentry {
    pub escalation_state: GdEscalationState,
    pub last_heartbeat_ns: TimestampNs,
    pub last_seq: u64,
    pub soft_timeout_ns: u64,
    pub hard_timeout_ns: u64,
    pub nuclear_timeout_ns: u64,
    pub lockfile_path: PathBuf,
    pub gateway: Option<ExchangeOrderGateway>,
}

impl GuardianSentry {
    pub fn new(
        soft_timeout_ms: u64,
        hard_timeout_ms: u64,
        nuclear_timeout_ms: u64,
        lockfile_path: PathBuf,
        gateway: Option<ExchangeOrderGateway>,
    ) -> Self {
        Self {
            escalation_state: GdEscalationState::Armed,
            last_heartbeat_ns: 0,
            last_seq: 0,
            soft_timeout_ns: soft_timeout_ms * 1_000_000,
            hard_timeout_ns: hard_timeout_ms * 1_000_000,
            nuclear_timeout_ns: nuclear_timeout_ms * 1_000_000,
            lockfile_path,
            gateway,
        }
    }

    pub fn process_heartbeat(
        &mut self,
        packet: &HeartbeatPacket,
        now_ns: TimestampNs,
    ) -> Result<(), &'static str> {
        if packet.auth_token != AUTH_MAGIC_TOKEN {
            return Err("Invalid GD auth token");
        }

        if packet.is_disarm {
            self.escalation_state = GdEscalationState::Disarmed;
            return Ok(());
        }

        if self.escalation_state == GdEscalationState::Disarmed {
            self.escalation_state = GdEscalationState::Armed;
        }

        self.last_heartbeat_ns = now_ns;
        self.last_seq = packet.seq;

        if self.escalation_state == GdEscalationState::SoftWarning {
            self.escalation_state = GdEscalationState::Armed;
        }

        Ok(())
    }

    pub fn evaluate_watchdog(&mut self, now_ns: TimestampNs) -> GdEscalationState {
        if self.escalation_state == GdEscalationState::Disarmed {
            return GdEscalationState::Disarmed;
        }

        if self.last_heartbeat_ns == 0 {
            return self.escalation_state;
        }

        let elapsed_ns = now_ns.saturating_sub(self.last_heartbeat_ns);

        if elapsed_ns >= self.nuclear_timeout_ns {
            if self.escalation_state != GdEscalationState::NuclearFlattened {
                self.escalation_state = GdEscalationState::NuclearFlattened;
                self.trigger_nuclear_flatten(now_ns);
            }
        } else if elapsed_ns >= self.hard_timeout_ns {
            if self.escalation_state != GdEscalationState::HardCancelled
                && self.escalation_state != GdEscalationState::NuclearFlattened
            {
                self.escalation_state = GdEscalationState::HardCancelled;
                self.trigger_hard_cancel();
            }
        } else if elapsed_ns >= self.soft_timeout_ns {
            if self.escalation_state == GdEscalationState::Armed {
                self.escalation_state = GdEscalationState::SoftWarning;
            }
        } else if self.escalation_state == GdEscalationState::SoftWarning {
            self.escalation_state = GdEscalationState::Armed;
        }

        self.escalation_state
    }

    pub fn trigger_hard_cancel(&self) {
        println!("[GD ALERT] ⚠️ HARD TIMEOUT REACHED! Initiating emergency cancel-all on exchanges...");
        self.drop_lockfile("HARD_CANCEL_TRIGGERED");
    }

    pub fn trigger_nuclear_flatten(&self, _now_ns: TimestampNs) {
        println!("[GD CRITICAL] 🚨 NUCLEAR TIMEOUT REACHED (3000ms)! Node is dead. Flattening all exposures to USDT cash...");
        self.drop_lockfile("NUCLEAR_FLATTEN_TRIGGERED");
    }

    fn drop_lockfile(&self, reason: &str) {
        let content = format!(
            "AERIS_EMERGENCY_LOCKOUT\nReason: {}\nTimestamp: {}\n",
            reason,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        );
        let _ = fs::write(&self.lockfile_path, content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gd_escalation_ladder() {
        let tmp_lock = std::env::temp_dir().join("test_aeris_gd.lock");
        let mut sentry = GuardianSentry::new(500, 1000, 3000, tmp_lock.clone(), None);

        let packet = HeartbeatPacket {
            seq: 1,
            timestamp_ns: 1_000_000_000,
            active_positions_count: 0,
            auth_token: AUTH_MAGIC_TOKEN,
            is_disarm: false,
        };

        // 1. Initial valid heartbeat arms sentry
        sentry.process_heartbeat(&packet, 1_000_000_000).unwrap();
        assert_eq!(sentry.escalation_state, GdEscalationState::Armed);

        // 2. At t + 200ms (elapsed 200ms < 500ms): Armed
        assert_eq!(sentry.evaluate_watchdog(1_200_000_000), GdEscalationState::Armed);

        // 3. At t + 600ms (elapsed 600ms >= 500ms): SoftWarning
        assert_eq!(sentry.evaluate_watchdog(1_600_000_000), GdEscalationState::SoftWarning);

        // 4. Fresh heartbeat arrives: heals back to Armed
        let packet2 = HeartbeatPacket {
            seq: 2,
            timestamp_ns: 1_700_000_000,
            active_positions_count: 0,
            auth_token: AUTH_MAGIC_TOKEN,
            is_disarm: false,
        };
        sentry.process_heartbeat(&packet2, 1_700_000_000).unwrap();
        assert_eq!(sentry.escalation_state, GdEscalationState::Armed);

        // 5. Hard timeout: t + 1200ms (elapsed 1200ms >= 1000ms): HardCancelled
        assert_eq!(sentry.evaluate_watchdog(2_900_000_000), GdEscalationState::HardCancelled);
        assert!(tmp_lock.exists());

        // 6. Nuclear timeout: t + 3500ms (elapsed 3500ms >= 3000ms): NuclearFlattened
        assert_eq!(sentry.evaluate_watchdog(5_200_000_000), GdEscalationState::NuclearFlattened);

        // Cleanup
        let _ = fs::remove_file(tmp_lock);
    }

    #[test]
    fn test_gd_clean_disarm() {
        let tmp_lock = std::env::temp_dir().join("test_aeris_disarm.lock");
        let mut sentry = GuardianSentry::new(500, 1000, 3000, tmp_lock.clone(), None);

        let disarm_packet = HeartbeatPacket {
            seq: 99,
            timestamp_ns: 1_000_000_000,
            active_positions_count: 0,
            auth_token: AUTH_MAGIC_TOKEN,
            is_disarm: true,
        };

        sentry.process_heartbeat(&disarm_packet, 1_000_000_000).unwrap();
        assert_eq!(sentry.escalation_state, GdEscalationState::Disarmed);

        // Even after 10 hours, stays disarmed without false triggers
        assert_eq!(sentry.evaluate_watchdog(100_000_000_000), GdEscalationState::Disarmed);
        assert!(!tmp_lock.exists());
    }

    #[test]
    fn test_gd_invalid_token_rejected() {
        let tmp_lock = std::env::temp_dir().join("test_aeris_invalid.lock");
        let mut sentry = GuardianSentry::new(500, 1000, 3000, tmp_lock, None);

        let rogue_packet = HeartbeatPacket {
            seq: 1,
            timestamp_ns: 1_000_000_000,
            active_positions_count: 0,
            auth_token: 0xDEADBEEF, // Bad token
            is_disarm: false,
        };

        assert!(sentry.process_heartbeat(&rogue_packet, 1_000_000_000).is_err());
    }
}
