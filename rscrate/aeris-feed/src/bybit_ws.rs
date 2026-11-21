use aeris_core::{BookLevel, BookUpdate, Fixed8, Symbol, Venue};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;

#[derive(Error, Debug)]
pub enum BybitWsError {
    #[error("WebSocket connection error: {0}")]
    ConnectionError(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("JSON parsing error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Stream terminated")]
    Terminated,
}

#[derive(Deserialize, Debug)]
struct BybitBookData {
    #[serde(default)]
    b: Vec<[String; 2]>,
    #[serde(default)]
    a: Vec<[String; 2]>,
}

#[derive(Deserialize, Debug)]
struct BybitDepthPayload {
    #[serde(rename = "type", default)]
    update_type: String,
    #[serde(rename = "ts", default)]
    timestamp_ms: u64,
    #[serde(default)]
    data: Option<BybitBookData>,
}

pub fn parse_bybit_depth(json_str: &str, symbol: Symbol) -> Result<Option<BookUpdate>, BybitWsError> {
    let payload: BybitDepthPayload = serde_json::from_str(json_str)?;

    let data = match payload.data {
        Some(d) => d,
        None => return Ok(None),
    };

    let mut bids = Vec::with_capacity(data.b.len());
    for b in data.b {
        if let (Ok(px), Ok(qty)) = (b[0].parse::<f64>(), b[1].parse::<f64>()) {
            bids.push(BookLevel {
                price: Fixed8::from_f64(px),
                qty: Fixed8::from_f64(qty),
            });
        }
    }

    let mut asks = Vec::with_capacity(data.a.len());
    for a in data.a {
        if let (Ok(px), Ok(qty)) = (a[0].parse::<f64>(), a[1].parse::<f64>()) {
            asks.push(BookLevel {
                price: Fixed8::from_f64(px),
                qty: Fixed8::from_f64(qty),
            });
        }
    }

    let is_snapshot = payload.update_type == "snapshot";

    Ok(Some(BookUpdate {
        venue: Venue::Bybit,
        symbol,
        timestamp_ns: payload.timestamp_ms * 1_000_000,
        bids,
        asks,
        is_snapshot,
    }))
}

pub async fn connect_bybit_depth(
    symbol_str: &str,
    tx: mpsc::Sender<BookUpdate>,
) -> Result<(), BybitWsError> {
    crate::init_crypto_provider();
    let symbol = Symbol::from_str_truncate(symbol_str);
    let url = "wss://stream.bybit.com/v5/public/linear";

    let (ws_stream, _) = connect_async(url).await?;
    let (mut write, mut read) = ws_stream.split();

    let sub_msg = serde_json::json!({
        "op": "subscribe",
        "args": [format!("orderbook.50.{}", symbol_str)]
    })
    .to_string();

    write
        .send(tokio_tungstenite::tungstenite::Message::Text(sub_msg))
        .await?;

    while let Some(msg) = read.next().await {
        match msg {
            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                if let Ok(Some(update)) = parse_bybit_depth(&text, symbol) {
                    if tx.send(update).await.is_err() {
                        break;
                    }
                }
            }
            Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => break,
            Err(e) => return Err(BybitWsError::ConnectionError(e)),
            _ => {}
        }
    }

    Ok(())
}
