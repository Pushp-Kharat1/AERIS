use aeris_core::{Fixed8, Side};

#[derive(Clone, Copy, Debug)]
pub struct FrictionModel {
    pub maker_fee_bps: Fixed8,
    pub taker_fee_bps: Fixed8,
    pub tds_rate: Fixed8,
}

impl Default for FrictionModel {
    fn default() -> Self {
        Self {
            maker_fee_bps: Fixed8::from_f64(2.0),
            taker_fee_bps: Fixed8::from_f64(5.0),
            tds_rate: Fixed8::from_f64(0.01),
        }
    }
}

impl FrictionModel {
    #[inline(always)]
    pub fn compute_fee(&self, notional: Fixed8, is_maker: bool) -> Fixed8 {
        let fee_bps = if is_maker {
            self.maker_fee_bps
        } else {
            self.taker_fee_bps
        };
        (notional * fee_bps) / Fixed8::from_f64(10000.0)
    }

    #[inline(always)]
    pub fn compute_tds(&self, notional: Fixed8, side: Side) -> Fixed8 {
        match side {
            Side::Sell => notional * self.tds_rate,
            Side::Buy => Fixed8::ZERO,
        }
    }

    #[inline(always)]
    pub fn compute_roundtrip_friction(&self, notional: Fixed8, is_taker: bool) -> Fixed8 {
        let fee_per_leg = self.compute_fee(notional, !is_taker);
        let fees = fee_per_leg + fee_per_leg;
        let tds = self.compute_tds(notional, Side::Sell);
        fees + tds
    }
}
