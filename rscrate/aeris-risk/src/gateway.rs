use aeris_core::{Fixed8, OrderIntent};
use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum RiskRejection {
    #[error("Order notional {notional} exceeds fat-finger max limit {max_limit}")]
    FatFingerLimitExceeded { notional: Fixed8, max_limit: Fixed8 },

    #[error("Projected portfolio leverage {projected_leverage} exceeds hard cap {max_leverage}")]
    LeverageCapExceeded { projected_leverage: Fixed8, max_leverage: Fixed8 },

    #[error("Order price {order_price} deviates {deviation_bps} bps from BBO {bbo_price}, exceeding max limit {max_bps} bps")]
    PriceDeviationExceeded {
        order_price: Fixed8,
        bbo_price: Fixed8,
        deviation_bps: u32,
        max_bps: u32,
    },

    #[error("Account has zero or negative equity {equity}")]
    InsolventAccount { equity: Fixed8 },

    #[error("Meta-label win probability {probability} is below threshold hurdle {min_hurdle}")]
    MetaLabelHurdleFailed { probability: Fixed8, min_hurdle: Fixed8 },
}

#[derive(Clone, Debug)]
pub struct RiskGatewayConfig {
    pub max_leverage: Fixed8,
    pub max_order_notional: Fixed8,
    pub max_price_deviation_bps: u32,
}

impl Default for RiskGatewayConfig {
    fn default() -> Self {
        Self {
            max_leverage: Fixed8::from_f64(2.5),
            max_order_notional: Fixed8::from_f64(100_000.0),
            max_price_deviation_bps: 300,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AccountRiskState {
    pub total_equity: Fixed8,
    pub total_deployed_notional: Fixed8,
}

pub struct PreTradeRiskGateway {
    config: RiskGatewayConfig,
}

impl PreTradeRiskGateway {
    pub fn new(config: RiskGatewayConfig) -> Self {
        Self { config }
    }

    pub fn evaluate_order(
        &self,
        intent: &OrderIntent,
        account: &AccountRiskState,
        current_bbo_price: Option<Fixed8>,
    ) -> Result<(), RiskRejection> {
        if account.total_equity <= Fixed8::ZERO {
            return Err(RiskRejection::InsolventAccount {
                equity: account.total_equity,
            });
        }

        let order_notional = intent.price * intent.qty;
        if order_notional > self.config.max_order_notional {
            return Err(RiskRejection::FatFingerLimitExceeded {
                notional: order_notional,
                max_limit: self.config.max_order_notional,
            });
        }

        let new_deployed = account.total_deployed_notional + order_notional;
        let projected_leverage = new_deployed / account.total_equity;
        if projected_leverage > self.config.max_leverage {
            return Err(RiskRejection::LeverageCapExceeded {
                projected_leverage,
                max_leverage: self.config.max_leverage,
            });
        }

        if let Some(bbo) = current_bbo_price {
            if !bbo.is_zero() {
                let diff = (intent.price - bbo).abs();
                let bps = (diff / bbo) * Fixed8::from_f64(10000.0);
                let bps_u32 = bps.to_f64().round() as u32;
                if bps_u32 > self.config.max_price_deviation_bps {
                    return Err(RiskRejection::PriceDeviationExceeded {
                        order_price: intent.price,
                        bbo_price: bbo,
                        deviation_bps: bps_u32,
                        max_bps: self.config.max_price_deviation_bps,
                    });
                }
            }
        }

        Ok(())
    }
}
