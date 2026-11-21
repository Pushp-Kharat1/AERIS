use std::env;

#[derive(Clone)]
pub struct ExchangeCredentials {
    pub api_key: String,
    pub api_secret: String,
    pub is_testnet: bool,
}

impl ExchangeCredentials {
    pub fn new(api_key: String, api_secret: String, is_testnet: bool) -> Self {
        Self {
            api_key,
            api_secret,
            is_testnet,
        }
    }

    pub fn from_env(prefix: &str, is_testnet: bool) -> Option<Self> {
        let key_var = format!("{}_API_KEY", prefix.to_uppercase());
        let sec_var = format!("{}_API_SECRET", prefix.to_uppercase());

        let api_key = env::var(&key_var).ok()?;
        let api_secret = env::var(&sec_var).ok()?;

        if api_key.trim().is_empty() || api_secret.trim().is_empty() {
            return None;
        }

        Some(Self {
            api_key: api_key.trim().to_string(),
            api_secret: api_secret.trim().to_string(),
            is_testnet,
        })
    }

    pub fn binance_rest_base_url(&self) -> &'static str {
        if self.is_testnet {
            "https://testnet.binancefuture.com"
        } else {
            "https://fapi.binance.com"
        }
    }

    pub fn bybit_rest_base_url(&self) -> &'static str {
        if self.is_testnet {
            "https://api-testnet.bybit.com"
        } else {
            "https://api.bybit.com"
        }
    }
}
