use aeris_core::{Fixed8, OrderIntent};
use crate::gateway::RiskRejection;

#[derive(Clone, Debug)]
pub struct MetaLabelGateConfig {
    pub min_probability_hurdle: Fixed8,
    pub min_kelly_fraction: Fixed8,
    pub max_kelly_fraction: Fixed8,
}

impl Default for MetaLabelGateConfig {
    fn default() -> Self {
        Self {
            min_probability_hurdle: Fixed8::from_f64(0.60),
            min_kelly_fraction: Fixed8::from_f64(0.20),
            max_kelly_fraction: Fixed8::from_f64(1.00),
        }
    }
}

pub struct MetaLabelGate {
    config: MetaLabelGateConfig,
}

impl MetaLabelGate {
    pub fn new(config: MetaLabelGateConfig) -> Self {
        Self { config }
    }

    pub fn evaluate_and_size(
        &self,
        intent: &OrderIntent,
        predicted_win_prob: Fixed8,
    ) -> Result<OrderIntent, RiskRejection> {
        if predicted_win_prob < self.config.min_probability_hurdle {
            return Err(RiskRejection::MetaLabelHurdleFailed {
                probability: predicted_win_prob,
                min_hurdle: self.config.min_probability_hurdle,
            });
        }

        let raw_kelly = (predicted_win_prob * Fixed8::from_f64(2.0)) - Fixed8::ONE;
        let kelly = raw_kelly
            .max(self.config.min_kelly_fraction)
            .min(self.config.max_kelly_fraction);

        let mut sized_intent = *intent;
        sized_intent.qty = intent.qty * kelly;
        Ok(sized_intent)
    }
}
