use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::{Duration, Instant};

use serde::Deserialize;

// Day 7 of the Rust port: measure a *basket*, not a single pair. The Python
// monitor tracks execution cost across tokens of different liquidity — majors
// quote tight, midcaps don't — so one pair hides the metric that matters.
// Each token gets its own latency / implied-price / price-impact / route
// summary from the same $100 USDC notional.
//
// Timing note (Day 2): `send()` returns at response headers, so the clock
// wraps send + full body parse to match the Python original (requests.get).

const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const USDC_DECIMALS: u32 = 6;

/// A quote target: spend a fixed USDC notional to buy `symbol`.
struct Token {
    symbol: &'static str,
    mint: &'static str,
    decimals: u32,
}

// A major, a large-cap and a midcap — deliberately different liquidity so the
// per-token spread/impact/route-churn actually diverge.
const BASKET: &[Token] = &[
    Token { symbol: "wSOL", mint: "So11111111111111111111111111111111111111112", decimals: 9 },
    Token { symbol: "JUP",  mint: "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN", decimals: 6 },
    Token { symbol: "BONK", mint: "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263", decimals: 5 },
];

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
    label: Option<String>,
}

impl QuoteResponse {
    /// USDC paid per whole output token received. `out_decimals` varies per
    /// token, so it is passed in rather than assumed (was hard-coded to SOL).
    fn implied_price_usd(&self, out_decimals: u32) -> Result<f64, std::num::ParseIntError> {
        let usdc = self.in_amount.parse::<u64>()? as f64 / 10f64.powi(USDC_DECIMALS as i32);
        let tok = self.out_amount.parse::<u64>()? as f64 / 10f64.powi(out_decimals as i32);
        Ok(usdc / tok)
    }

    fn route_signature(&self) -> String {
        self.route_plan
            .iter()
            .map(|step| {
                let label = step.swap_info.label.as_deref().unwrap_or("unknown");
                format!("{label} ({}%)", step.percent)
            })
            .collect::<Vec<_>>()
            .join(" -> ")
    }
}

/// One quote's outcome, kept so the run can be written out and re-checked.
struct Record {
    symbol: &'static str,
    idx: usize,
    latency_ms: u128,
    price: Option<f64>,
    route: String,
    ok: bool,
}

fn write_csv(path: &str, records: &[Record]) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
    writeln!(w, "symbol,idx,latency_ms,price_usd,route,ok")?;
    for r in records {
        let price = r.price.map(|p| format!("{p:.6}")).unwrap_or_default();
        // route can contain commas — wrap it in quotes so the CSV stays valid
        writeln!(
            w,
            "{},{},{},{},\"{}\",{}",
            r.symbol, r.idx, r.latency_ms, price, r.route, r.ok
        )?;
    }
    Ok(())
}

fn percentile(sorted: &[u128], p: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p / 100.0).round() as usize;
    sorted[idx]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // usage: solana-quote-bench [n_per_token] [out.csv]
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    let csv_path = std::env::args().nth(2);

    let amount: u64 = 100_000_000; // $100 in USDC atomic units (6 decimals)

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    println!(
        "solana-quote-bench: {n} quotes x {} tokens ($100 USDC notional)\n",
        BASKET.len()
    );

    let mut all_records: Vec<Record> = Vec::with_capacity(n * BASKET.len());

    for token in BASKET {
        let url = format!(
            "https://lite-api.jup.ag/swap/v1/quote?inputMint={USDC}&outputMint={}&amount={amount}&slippageBps=50",
            token.mint
        );

        let mut latencies: Vec<u128> = Vec::with_capacity(n);
        let mut prices: Vec<f64> = Vec::with_capacity(n);
        let mut impacts: Vec<f64> = Vec::with_capacity(n);
        let mut routes: HashMap<String, usize> = HashMap::new();
        let mut failures = 0usize;

        println!("== {} ==", token.symbol);
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
                    let price = quote.implied_price_usd(token.decimals)?;
                    let route = quote.route_signature();
                    // priceImpactPct arrives as a decimal fraction (0.001 = 0.1%)
                    impacts.push(quote.price_impact_pct.parse().unwrap_or(0.0));
                    latencies.push(ms);
                    prices.push(price);
                    *routes.entry(route.clone()).or_insert(0) += 1;
                    all_records.push(Record { symbol: token.symbol, idx: i, latency_ms: ms, price: Some(price), route, ok: true });
                }
                Err(e) => {
                    failures += 1;
                    all_records.push(Record { symbol: token.symbol, idx: i, latency_ms: ms, price: None, route: String::new(), ok: false });
                    eprintln!("  #{i:02} FAIL: {e}");
                }
            }
        }

        latencies.sort_unstable();
        let ok = latencies.len();
        if ok > 0 {
            let sum: u128 = latencies.iter().sum();
            println!(
                "  latency: p50 {} · p90 {} · p99 {} · mean {} ms   ({ok} ok / {failures} fail)",
                percentile(&latencies, 50.0),
                percentile(&latencies, 90.0),
                percentile(&latencies, 99.0),
                sum / ok as u128,
            );
        } else {
            println!("  latency: no successful quotes ({failures} fail)");
        }

        if !prices.is_empty() {
            let lo = prices.iter().cloned().fold(f64::INFINITY, f64::min);
            let hi = prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let mean = prices.iter().sum::<f64>() / prices.len() as f64;
            let mean_impact = impacts.iter().sum::<f64>() / impacts.len() as f64;
            println!(
                "  implied price: mean ${mean:.6} · spread {:.3}% · mean impact {:.4}%",
                (hi - lo) / mean * 100.0,
                mean_impact * 100.0
            );
        }

        println!("  routes: {} distinct", routes.len());
        let mut by_count: Vec<(&String, &usize)> = routes.iter().collect();
        by_count.sort_by(|a, b| b.1.cmp(a.1));
        for (route, count) in by_count.into_iter().take(3) {
            println!("    {count:>3}x  {route}");
        }
        println!();
    }

    if let Some(path) = csv_path {
        write_csv(&path, &all_records)?;
        println!("wrote {} rows -> {path}", all_records.len());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::percentile;

    #[test]
    fn percentile_picks_expected_values() {
        let xs = vec![10u128, 20, 30, 40, 50];
        assert_eq!(percentile(&xs, 0.0), 10);
        assert_eq!(percentile(&xs, 50.0), 30);
        assert_eq!(percentile(&xs, 100.0), 50);
    }

    #[test]
    fn percentile_of_empty_is_zero() {
        assert_eq!(percentile(&[], 50.0), 0);
    }

    #[test]
    fn percentile_of_single_element() {
        assert_eq!(percentile(&[42u128], 99.0), 42);
    }

    #[test]
    fn implied_price_from_known_amounts() {
        // 100 USDC in (6 decimals) for exactly 1 SOL out (9 decimals) => $100/SOL
        let q = super::QuoteResponse {
            in_amount: "100000000".to_string(),
            out_amount: "1000000000".to_string(),
            price_impact_pct: "0".to_string(),
            route_plan: vec![],
        };
        let price = q.implied_price_usd(9).unwrap();
        assert!((price - 100.0).abs() < 1e-9, "expected ~100, got {price}");
    }
}
