use aeris_core::{ExecutionReport, ExecutionStatus, Fixed8, OrderIntent, TimestampNs};
use aeris_risk::{AccountRiskState, PreTradeRiskGateway, RiskRejection};
use crate::friction::FrictionModel;

pub struct ExecutionRouter {
    risk_gateway: PreTradeRiskGateway,
    friction: FrictionModel,
}

impl ExecutionRouter {
    pub fn new(risk_gateway: PreTradeRiskGateway, friction: FrictionModel) -> Self {
        Self {
            risk_gateway,
            friction,
        }
    }

    pub fn validate_and_route(
        &self,
        intent: &OrderIntent,
        account: &AccountRiskState,
        bbo_price: Option<Fixed8>,
    ) -> Result<OrderIntent, RiskRejection> {
        self.risk_gateway.evaluate_order(intent, account, bbo_price)?;
        Ok(*intent)
    }

    pub fn process_fill(
        &self,
        intent: &OrderIntent,
        fill_price: Fixed8,
        fill_qty: Fixed8,
        is_maker: bool,
        timestamp_ns: TimestampNs,
    ) -> ExecutionReport {
        let notional = fill_price * fill_qty;
        let fee = self.friction.compute_fee(notional, is_maker);
        let tds = self.friction.compute_tds(notional, intent.side);

        ExecutionReport {
            client_order_id: intent.client_order_id,
            venue: intent.venue,
            symbol: intent.symbol,
            status: ExecutionStatus::Filled,
            last_fill_price: fill_price,
            last_fill_qty: fill_qty,
            cumulative_qty: fill_qty,
            leaves_qty: intent.qty - fill_qty,
            fee_paid: fee,
            tds_paid: tds,
            updated_at_ns: timestamp_ns,
        }
    }
}
