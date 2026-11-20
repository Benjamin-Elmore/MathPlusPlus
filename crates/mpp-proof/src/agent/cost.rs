//! The live cost counter and the spend cap.

/// List prices in US dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prices {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

impl Prices {
    /// Claude Opus 5.5 list prices: $4 in, $20 out, $0.20 cache reads; cache writes at
    /// 1.25 × input (the five-minute cache).
    pub const OPUS_5_5: Prices = Prices {
        input: 4.0,
        output: 20.0,
        cache_read: 0.20,
        cache_write: 5.0,
    };

    /// The prices for a model id (unknown models are priced as Opus 5.5, the dearest the
    /// client uses, so the estimate never runs low).
    pub fn for_model(_model: &str) -> Prices {
        Prices::OPUS_5_5
    }
}

/// Token counts and dollars spent in one run.
#[derive(Debug, Clone, PartialEq)]
pub struct CostMeter {
    pub prices: Prices,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// The authoritative cost reported by the server (session list cost), when known.
    pub reported_usd: Option<f64>,
    /// The per-run cap.
    pub cap_usd: f64,
}

impl CostMeter {
    pub fn new(prices: Prices, cap_usd: f64) -> Self {
        CostMeter {
            prices,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reported_usd: None,
            cap_usd,
        }
    }

    /// Adds one model request's usage.
    pub fn add(&mut self, input: u64, output: u64, cache_read: u64, cache_write: u64) {
        self.input_tokens += input;
        self.output_tokens += output;
        self.cache_read_tokens += cache_read;
        self.cache_write_tokens += cache_write;
    }

    /// The cost estimated from the tokens alone.
    pub fn estimated_usd(&self) -> f64 {
        let p = self.prices;
        (self.input_tokens as f64 * p.input
            + self.output_tokens as f64 * p.output
            + self.cache_read_tokens as f64 * p.cache_read
            + self.cache_write_tokens as f64 * p.cache_write)
            / 1_000_000.0
    }

    /// The cost shown in the counter: the larger of the server's figure and the estimate.
    pub fn usd(&self) -> f64 {
        self.reported_usd
            .map_or(self.estimated_usd(), |r| r.max(self.estimated_usd()))
    }

    /// True once the run has spent its cap.
    pub fn over_cap(&self) -> bool {
        self.usd() >= self.cap_usd
    }

    /// The cap in cents as the server wants it (an integer string, at least 1).
    pub fn cap_cents(&self) -> String {
        let cents = (self.cap_usd * 100.0).floor();
        if cents.is_finite() && cents >= 1.0 {
            format!("{}", cents as u64)
        } else {
            "1".to_string()
        }
    }

    /// The counter's text: `$0.42 of $2.00`.
    pub fn label(&self) -> String {
        format!("${:.2} of ${:.2}", self.usd(), self.cap_usd)
    }
}
