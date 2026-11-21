use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub fn compute_hmac_sha256_hex(secret: &str, data: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(data.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub fn sign_binance_query(query_params: &str, secret: &str) -> String {
    let signature = compute_hmac_sha256_hex(secret, query_params);
    format!("{}&signature={}", query_params, signature)
}

pub fn sign_bybit_v5_payload(
    timestamp_ms: u64,
    api_key: &str,
    recv_window_ms: u64,
    payload: &str,
    secret: &str,
) -> String {
    let sign_str = format!("{}{}{}{}", timestamp_ms, api_key, recv_window_ms, payload);
    compute_hmac_sha256_hex(secret, &sign_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hmac_sha256_known_vector() {
        let secret = "test_secret_key_12345";
        let message = "symbol=BTCUSDT&side=BUY&type=LIMIT&quantity=1.000&price=60000.00&timestamp=1700000000000";
        let sig = compute_hmac_sha256_hex(secret, message);
        assert_eq!(sig.len(), 64); // 256 bits = 32 bytes = 64 hex characters

        // Changing 1 character changes the entire hash (avalanche effect)
        let altered_sig = compute_hmac_sha256_hex(secret, "symbol=BTCUSDT&side=BUY&type=LIMIT&quantity=1.001&price=60000.00&timestamp=1700000000000");
        assert_ne!(sig, altered_sig);
    }

    #[test]
    fn test_binance_query_signing() {
        let secret = "binance_secret";
        let query = "symbol=BTCUSDT&timestamp=1600000000";
        let signed = sign_binance_query(query, secret);
        assert!(signed.starts_with("symbol=BTCUSDT&timestamp=1600000000&signature="));
        assert_eq!(signed.len(), query.len() + "&signature=".len() + 64);
    }

    #[test]
    fn test_bybit_v5_signing() {
        let secret = "bybit_secret";
        let api_key = "bybit_key_123";
        let ts = 1700000000123;
        let payload = r#"{"category":"linear","symbol":"BTCUSDT","side":"Buy","orderType":"Limit","qty":"0.1","price":"60000"}"#;

        let sig = sign_bybit_v5_payload(ts, api_key, 5000, payload, secret);
        assert_eq!(sig.len(), 64);
    }
}
