use aeris_core::{BookLevel, BookUpdate, Fixed8, Symbol, Venue};
use futures_util::StreamExt;
use serde::Deserialize;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;

#[derive(Error, Debug)]
pub enum BinanceWsError {
    #[error("WebSocket connection error: {0}")]
    ConnectionError(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("JSON parsing error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Stream terminated")]
    Terminated,
}

#[derive(Deserialize, Debug)]
struct BinanceDepthPayload {
    #[serde(rename = "E", default)]
    event_time: u64,
    #[serde(rename = "b", alias = "bids", default)]
    bids: Vec<[String; 2]>,
    #[serde(rename = "a", alias = "asks", default)]
    asks: Vec<[String; 2]>,
}

pub fn parse_binance_depth(json_str: &str, symbol: Symbol) -> Result<BookUpdate, BinanceWsError> {
    let payload: BinanceDepthPayload = serde_json::from_str(json_str)?;

    let mut bids = Vec::with_capacity(payload.bids.len());
    for b in payload.bids {
        if let (Ok(px), Ok(qty)) = (b[0].parse::<f64>(), b[1].parse::<f64>()) {
            bids.push(BookLevel {
                price: Fixed8::from_f64(px),
                qty: Fixed8::from_f64(qty),
            });
        }
    }

    let mut asks = Vec::with_capacity(payload.asks.len());
    for a in payload.asks {
        if let (Ok(px), Ok(qty)) = (a[0].parse::<f64>(), a[1].parse::<f64>()) {
            asks.push(BookLevel {
                price: Fixed8::from_f64(px),
                qty: Fixed8::from_f64(qty),
            });
        }
    }

    Ok(BookUpdate {
        venue: Venue::Binance,
        symbol,
        timestamp_ns: payload.event_time * 1_000_000,
        bids,
        asks,
        is_snapshot: true,
    })
}

pub async fn connect_binance_depth(
    symbol_str: &str,
    tx: mpsc::Sender<BookUpdate>,
) -> Result<(), BinanceWsError> {
    crate::init_crypto_provider();
    let sym_lower = symbol_str.to_lowercase();
    let symbol = Symbol::from_str_truncate(symbol_str);
    let url = format!(
        "wss://fstream.binance.com/ws/{}@depth20@100ms",
        sym_lower
    );

    let (ws_stream, _) = connect_async(&url).await?;
    let (_, mut read) = ws_stream.split();

    while let Some(msg) = read.next().await {
        match msg {
            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                if let Ok(update) = parse_binance_depth(&text, symbol) {
                    if tx.send(update).await.is_err() {
                        break;
                    }
                }
            }
            Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => break,
            Err(e) => return Err(BinanceWsError::ConnectionError(e)),
            _ => {}
        }
    }

    Ok(())
}
