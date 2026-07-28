use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::{Duration, Instant};

use serde::Deserialize;

// Day 4 of the Rust port: optionally persist each quote to CSV. A benchmark
// run you can't inspect afterwards isn't really a measurement — so the
// per-request records (latency, implied price, route) now survive the run.
//
// Timing note (Day 2): `send()` returns at response headers, so the clock
// wraps send + full body parse to match the Python original (requests.get).

const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const WSOL: &str = "So11111111111111111111111111111111111111112";
const USDC_DECIMALS: u32 = 6;
const WSOL_DECIMALS: u32 = 9;

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
    /// USDC paid per whole SOL received — the implied price the Python
    /// collector stores for cross-source reconciliation.
    fn implied_price_usd(&self) -> Result<f64, std::num::ParseIntError> {
        let usdc = self.in_amount.parse::<u64>()? as f64 / 10f64.powi(USDC_DECIMALS as i32);
        let sol = self.out_amount.parse::<u64>()? as f64 / 10f64.powi(WSOL_DECIMALS as i32);
        Ok(usdc / sol)
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
    idx: usize,
    latency_ms: u128,
    price: Option<f64>,
    route: String,
    ok: bool,
}

fn write_csv(path: &str, records: &[Record]) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
    writeln!(w, "idx,latency_ms,price_usd,route,ok")?;
    for r in records {
        let price = r.price.map(|p| format!("{p:.6}")).unwrap_or_default();
        // route can contain commas — wrap it in quotes so the CSV stays valid
        writeln!(
            w,
            "{},{},{},\"{}\",{}",
            r.idx, r.latency_ms, price, r.route, r.ok
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
    // usage: solana-quote-bench [n_requests] [out.csv]
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let csv_path = std::env::args().nth(2);

    let amount: u64 = 100_000_000; // $100 in USDC atomic units
    let url = format!(
        "https://lite-api.jup.ag/swap/v1/quote?inputMint={USDC}&outputMint={WSOL}&amount={amount}&slippageBps=50"
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    println!("solana-quote-bench: {n} Jupiter quotes ($100 USDC -> wSOL)\n");

    let mut records: Vec<Record> = Vec::with_capacity(n);
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
                let route = quote.route_signature();
                latencies.push(ms);
                prices.push(price);
                *routes.entry(route.clone()).or_insert(0) += 1;
                records.push(Record { idx: i, latency_ms: ms, price: Some(price), route, ok: true });
                println!("  #{i:02}  {ms:>5} ms   ${price:>8.4}/SOL   impact {}%", quote.price_impact_pct);
            }
            Err(e) => {
                failures += 1;
                records.push(Record { idx: i, latency_ms: ms, price: None, route: String::new(), ok: false });
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

    println!("\nroutes seen ({} distinct):", routes.len());
    let mut by_count: Vec<_> = routes.iter().collect();
    by_count.sort_by(|a, b| b.1.cmp(a.1));
    for (route, count) in by_count {
        println!("  {count:>3}x  {route}");
    }

    if let Some(path) = csv_path {
        write_csv(&path, &records)?;
        println!("\nwrote {} rows -> {path}", records.len());
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
}
