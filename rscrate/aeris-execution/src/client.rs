use crate::credentials::ExchangeCredentials;
use crate::signing::{sign_binance_query, sign_bybit_v5_payload};
use aeris_core::{
    ExecutionReport, ExecutionStatus, Fixed8, OrderIntent, OrderType, Side, TimeInForce,
    TimestampNs, Venue,
};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GatewayError {
    #[error("Missing exchange credentials for venue: {0:?}")]
    MissingCredentials(Venue),

    #[error("HTTP network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("Exchange returned API error status {status_code}: {error_msg}")]
    ExchangeError {
        status_code: u16,
        error_msg: String,
    },

    #[error("Failed to parse exchange response: {0}")]
    ParseError(String),
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn current_time_ns() -> TimestampNs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

pub struct ExchangeOrderGateway {
    http: reqwest::Client,
    pub binance_creds: Option<ExchangeCredentials>,
    pub bybit_creds: Option<ExchangeCredentials>,
    pub is_dry_run: bool,
}

impl ExchangeOrderGateway {
    pub fn new(
        binance_creds: Option<ExchangeCredentials>,
        bybit_creds: Option<ExchangeCredentials>,
        is_dry_run: bool,
    ) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(5000))
                .build()
                .unwrap(),
            binance_creds,
            bybit_creds,
            is_dry_run,
        }
    }

    pub fn auto_from_env(is_testnet: bool, is_dry_run: bool) -> Self {
        let binance = ExchangeCredentials::from_env("BINANCE", is_testnet);
        let bybit = ExchangeCredentials::from_env("BYBIT", is_testnet);
        Self::new(binance, bybit, is_dry_run)
    }

    pub async fn submit_order(&self, intent: &OrderIntent) -> Result<ExecutionReport, GatewayError> {
        if self.is_dry_run {
            return Ok(ExecutionReport {
                client_order_id: intent.client_order_id,
                venue: intent.venue,
                symbol: intent.symbol,
                status: ExecutionStatus::Filled,
                last_fill_price: intent.price,
                last_fill_qty: intent.qty,
                cumulative_qty: intent.qty,
                leaves_qty: Fixed8::ZERO,
                fee_paid: Fixed8::ZERO,
                tds_paid: Fixed8::ZERO,
                updated_at_ns: current_time_ns(),
            });
        }

        match intent.venue {
            Venue::Binance => self.submit_binance_order(intent).await,
            Venue::Bybit => self.submit_bybit_order(intent).await,
            Venue::Simulated => Ok(ExecutionReport {
                client_order_id: intent.client_order_id,
                venue: intent.venue,
                symbol: intent.symbol,
                status: ExecutionStatus::Filled,
                last_fill_price: intent.price,
                last_fill_qty: intent.qty,
                cumulative_qty: intent.qty,
                leaves_qty: Fixed8::ZERO,
                fee_paid: Fixed8::ZERO,
                tds_paid: Fixed8::ZERO,
                updated_at_ns: current_time_ns(),
            }),
            Venue::Hyperliquid => Err(GatewayError::MissingCredentials(Venue::Hyperliquid)),
        }
    }

    async fn submit_binance_order(&self, intent: &OrderIntent) -> Result<ExecutionReport, GatewayError> {
        let creds = self
            .binance_creds
            .as_ref()
            .ok_or(GatewayError::MissingCredentials(Venue::Binance))?;

        let side_str = match intent.side {
            Side::Buy => "BUY",
            Side::Sell => "SELL",
        };

        let type_str = match intent.order_type {
            OrderType::Limit => "LIMIT",
            OrderType::Market | OrderType::ImmediateOrCancel => "MARKET",
        };

        let tif_str = match intent.time_in_force {
            TimeInForce::ImmediateOrCancel => "IOC",
            TimeInForce::GoodTilCancelled => "GTC",
            TimeInForce::PostOnly => "GTX",
            TimeInForce::FillOrKill => "FOK",
        };

        let ts = current_time_ms();
        let query_base = format!(
            "symbol={}&side={}&type={}&timeInForce={}&quantity={:.4}&price={:.2}&newClientOrderId=aeris_{}&timestamp={}&recvWindow=5000",
            intent.symbol.as_str(),
            side_str,
            type_str,
            tif_str,
            intent.qty.to_f64(),
            intent.price.to_f64(),
            intent.client_order_id,
            ts
        );

        let signed_query = sign_binance_query(&query_base, &creds.api_secret);
        let url = format!("{}/fapi/v1/order?{}", creds.binance_rest_base_url(), signed_query);

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-MBX-APIKEY",
            HeaderValue::from_str(&creds.api_key).unwrap(),
        );

        let resp = self.http.post(&url).headers(headers).send().await?;
        let status = resp.status();
        let body_text = resp.text().await?;

        if !status.is_success() {
            return Err(GatewayError::ExchangeError {
                status_code: status.as_u16(),
                error_msg: body_text,
            });
        }

        let json: Value = serde_json::from_str(&body_text)
            .map_err(|e| GatewayError::ParseError(e.to_string()))?;

        let avg_price = json["avgPrice"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or_else(|| intent.price.to_f64());
        let executed_qty = json["executedQty"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or_else(|| intent.qty.to_f64());

        Ok(ExecutionReport {
            client_order_id: intent.client_order_id,
            venue: Venue::Binance,
            symbol: intent.symbol,
            status: ExecutionStatus::Filled,
            last_fill_price: Fixed8::from_f64(avg_price),
            last_fill_qty: Fixed8::from_f64(executed_qty),
            cumulative_qty: Fixed8::from_f64(executed_qty),
            leaves_qty: Fixed8::ZERO,
            fee_paid: Fixed8::ZERO,
            tds_paid: Fixed8::ZERO,
            updated_at_ns: current_time_ns(),
        })
    }

    async fn submit_bybit_order(&self, intent: &OrderIntent) -> Result<ExecutionReport, GatewayError> {
        let creds = self
            .bybit_creds
            .as_ref()
            .ok_or(GatewayError::MissingCredentials(Venue::Bybit))?;

        let side_str = match intent.side {
            Side::Buy => "Buy",
            Side::Sell => "Sell",
        };

        let type_str = match intent.order_type {
            OrderType::Limit => "Limit",
            OrderType::Market | OrderType::ImmediateOrCancel => "Market",
        };

        let tif_str = match intent.time_in_force {
            TimeInForce::ImmediateOrCancel => "IOC",
            TimeInForce::GoodTilCancelled => "GTC",
            TimeInForce::PostOnly => "PostOnly",
            TimeInForce::FillOrKill => "FOK",
        };

        let payload = serde_json::json!({
            "category": "linear",
            "symbol": intent.symbol.as_str(),
            "side": side_str,
            "orderType": type_str,
            "qty": format!("{:.4}", intent.qty.to_f64()),
            "price": format!("{:.2}", intent.price.to_f64()),
            "timeInForce": tif_str,
            "orderLinkId": format!("aeris_{}", intent.client_order_id)
        })
        .to_string();

        let ts = current_time_ms();
        let recv_window = 5000;
        let sig = sign_bybit_v5_payload(ts, &creds.api_key, recv_window, &payload, &creds.api_secret);

        let url = format!("{}/v5/order/create", creds.bybit_rest_base_url());

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("X-BAPI-API-KEY", HeaderValue::from_str(&creds.api_key).unwrap());
        headers.insert("X-BAPI-TIMESTAMP", HeaderValue::from_str(&ts.to_string()).unwrap());
        headers.insert("X-BAPI-RECV-WINDOW", HeaderValue::from_str(&recv_window.to_string()).unwrap());
        headers.insert("X-BAPI-SIGN", HeaderValue::from_str(&sig).unwrap());

        let resp = self
            .http
            .post(&url)
            .headers(headers)
            .body(payload)
            .send()
            .await?;

        let status = resp.status();
        let body_text = resp.text().await?;

        if !status.is_success() {
            return Err(GatewayError::ExchangeError {
                status_code: status.as_u16(),
                error_msg: body_text,
            });
        }

        let json: Value = serde_json::from_str(&body_text)
            .map_err(|e| GatewayError::ParseError(e.to_string()))?;

        let ret_code = json["retCode"].as_i64().unwrap_or(-1);
        if ret_code != 0 {
            return Err(GatewayError::ExchangeError {
                status_code: ret_code as u16,
                error_msg: json["retMsg"].as_str().unwrap_or("Unknown Bybit error").to_string(),
            });
        }

        Ok(ExecutionReport {
            client_order_id: intent.client_order_id,
            venue: Venue::Bybit,
            symbol: intent.symbol,
            status: ExecutionStatus::Filled,
            last_fill_price: intent.price,
            last_fill_qty: intent.qty,
            cumulative_qty: intent.qty,
            leaves_qty: Fixed8::ZERO,
            fee_paid: Fixed8::ZERO,
            tds_paid: Fixed8::ZERO,
            updated_at_ns: current_time_ns(),
        })
    }
}
