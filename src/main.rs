use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Deserialize;

// Day 3 of the Rust port: typed deserialization instead of dynamic JSON,
// plus the two things the Python collector actually derives from a quote —
// implied price and which route the fill would take.
//
// Timing note (Day 2): `send()` returns at response headers, so the clock
// wraps send + full body parse to match the Python original (requests.get).

const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const WSOL: &str = "So11111111111111111111111111111111111111112";
const USDC_DECIMALS: u32 = 6;
const WSOL_DECIMALS: u32 = 9;

// Jupiter sends amounts as JSON *strings*, not numbers: a u64 lamport amount
// can exceed JavaScript's safe integer range, so the API keeps full precision
// by quoting them. We parse them explicitly rather than trusting a float.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuoteResponse {
    in_amount: String,
    out_amount: String,
    price_impact_pct: String,
    route_plan: Vec<RoutePlanStep>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoutePlanStep {
    swap_info: SwapInfo,
    percent: u8,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwapInfo {
    // Not every venue reports a label — Option makes "missing" a case the
    // compiler forces us to handle, instead of a runtime surprise.
    label: Option<String>,
}

impl QuoteResponse {
    /// USDC paid per whole SOL received — the same implied price the Python
    /// collector stores for cross-source reconciliation.
    fn implied_price_usd(&self) -> Result<f64, std::num::ParseIntError> {
        let usdc = self.in_amount.parse::<u64>()? as f64 / 10f64.powi(USDC_DECIMALS as i32);
        let sol = self.out_amount.parse::<u64>()? as f64 / 10f64.powi(WSOL_DECIMALS as i32);
        Ok(usdc / sol)
    }

    /// "Meteora DLMM → Whirlpool (50%)" — a stable signature for churn counting.
    fn route_signature(&self) -> String {
        self.route_plan
            .iter()
            .map(|step| {
                let label = step.swap_info.label.as_deref().unwrap_or("unknown");
                format!("{label} ({}%)", step.percent)
            })
            .collect::<Vec<_>>()
            .join(" → ")
    }
}

fn percentile(sorted: &[u128], p: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p / 100.0).round() as usize;
    sorted[idx]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);

    let amount: u64 = 100_000_000; // $100 in USDC atomic units
    let url = format!(
        "https://lite-api.jup.ag/swap/v1/quote?inputMint={USDC}&outputMint={WSOL}&amount={amount}&slippageBps=50"
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    println!("solana-quote-bench: {n} Jupiter quotes ($100 USDC -> wSOL)\n");

    let mut latencies: Vec<u128> = Vec::with_capacity(n);
    let mut prices: Vec<f64> = Vec::with_capacity(n);
    let mut routes: HashMap<String, usize> = HashMap::new();
    let mut failures = 0usize;

    for i in 1..=n {
        let t0 = Instant::now();
        let result: Result<QuoteResponse, reqwest::Error> = client
            .get(&url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.json());
        let ms = t0.elapsed().as_millis();

        match result {
            Ok(quote) => {
                let price = quote.implied_price_usd()?;
                latencies.push(ms);
                prices.push(price);
                *routes.entry(quote.route_signature()).or_insert(0) += 1;
                println!(
                    "  #{i:02}  {ms:>5} ms   ${price:>8.4}/SOL   impact {}%",
                    quote.price_impact_pct
                );
            }
            Err(e) => {
                failures += 1;
                println!("  #{i:02}  {ms:>5} ms   FAIL: {e}");
            }
        }
    }

    latencies.sort_unstable();
    let ok = latencies.len();
    println!("\nlatency ({ok} ok / {failures} fail):");
    if ok > 0 {
        let sum: u128 = latencies.iter().sum();
        println!(
            "  min {} · p50 {} · p90 {} · p99 {} · max {} · mean {} ms",
            latencies[0],
            percentile(&latencies, 50.0),
            percentile(&latencies, 90.0),
            percentile(&latencies, 99.0),
            latencies[ok - 1],
            sum / ok as u128
        );
    }

    if !prices.is_empty() {
        let lo = prices.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let mean = prices.iter().sum::<f64>() / prices.len() as f64;
        println!("\nimplied price: mean ${mean:.4} · range ${lo:.4}-${hi:.4} · spread {:.3}%",
            (hi - lo) / mean * 100.0);
    }

    // Route churn: how often the best path changes between identical requests.
    // Same metric the Python monitor tracks — a churny route means the fill you
    // priced is not necessarily the fill you get.
    println!("\nroutes seen ({} distinct):", routes.len());
    let mut by_count: Vec<_> = routes.iter().collect();
    by_count.sort_by(|a, b| b.1.cmp(a.1));
    for (route, count) in by_count {
        println!("  {count:>3}x  {route}");
    }

    Ok(())
}
