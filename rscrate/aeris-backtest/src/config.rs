use aeris_core::Fixed8;

#[derive(Clone, Debug)]
pub struct BacktestConfig {
    /// Maker fee in bps (e.g. 2.0 bps for VIP 0 standard)
    pub maker_fee_bps: Fixed8,
    /// Taker fee in bps (e.g. 5.0 bps for VIP 0 standard)
    pub taker_fee_bps: Fixed8,
    /// Network round-trip latency in milliseconds (e.g. 95ms Mumbai-Tokyo, 1.5ms Tokyo VPS)
    pub network_latency_ms: u64,
    /// Adverse selection penalty in bps applied to maker fills during aggressive flow
    pub adverse_selection_bps: Fixed8,
    /// Initial account equity in USD
    pub initial_equity: Fixed8,
    /// Indian Section 194S 1% TDS mode toggle
    pub is_onshore: bool,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            maker_fee_bps: Fixed8::from_f64(2.0),
            taker_fee_bps: Fixed8::from_f64(5.0),
            network_latency_ms: 95,
            adverse_selection_bps: Fixed8::from_f64(2.5),
            initial_equity: Fixed8::from_f64(100_000.0),
            is_onshore: false,
        }
    }
}

impl BacktestConfig {
    /// Colocated Tokyo VPS profile (<1.5ms ping, VIP 0 fees)
    pub fn tokyo_colocated() -> Self {
        Self {
            network_latency_ms: 1,
            ..Default::default()
        }
    }

    /// Remote Mumbai profile (95ms ping, VIP 0 fees)
    pub fn remote_mumbai() -> Self {
        Self {
            network_latency_ms: 95,
            ..Default::default()
        }
    }

    /// Institutional MM profile (-0.5 bps maker rebate, 1.5ms colocated)
    pub fn institutional_mm() -> Self {
        Self {
            maker_fee_bps: Fixed8::from_f64(-0.5),
            taker_fee_bps: Fixed8::from_f64(3.0),
            network_latency_ms: 1,
            adverse_selection_bps: Fixed8::from_f64(1.0),
            ..Default::default()
        }
    }
}
