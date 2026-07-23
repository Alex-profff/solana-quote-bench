use std::time::{Duration, Instant};

// Day 2 of the Rust port: quote loop + latency percentiles.
//
// Honest fix from Day 1: `reqwest::blocking::get()` returns when response
// HEADERS arrive, so Day 1 measured time-to-headers. The Python original
// (`requests.get`) downloads the FULL BODY before returning. To preserve the
// original's semantics, timing now wraps send + body parse.

const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const WSOL: &str = "So11111111111111111111111111111111111111112";

fn percentile(sorted: &[u128], p: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p / 100.0).round() as usize;
    sorted[idx]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // usage: solana-quote-bench [n_requests]  (default 30)
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);

    let amount: u64 = 100_000_000; // 100 USDC in atomic units (6 decimals)
    let url = format!(
        "https://lite-api.jup.ag/swap/v1/quote?inputMint={USDC}&outputMint={WSOL}&amount={amount}&slippageBps=50"
    );

    // A Client reuses the connection (keep-alive), same as a Python
    // requests.Session — first request pays DNS+TLS, the rest don't.
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    println!("solana-quote-bench: {n} Jupiter quotes, full-body latency");

    let mut latencies: Vec<u128> = Vec::with_capacity(n);
    let mut failures = 0usize;
    let mut last_out: Option<String> = None;

    for i in 1..=n {
        let t0 = Instant::now();
        let result: Result<serde_json::Value, reqwest::Error> = client
            .get(&url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.json());
        let ms = t0.elapsed().as_millis();

        match result {
            Ok(body) => {
                latencies.push(ms);
                last_out = body["outAmount"].as_str().map(str::to_string);
                println!("  #{i:02}  {ms:>5} ms  ok");
            }
            Err(e) => {
                failures += 1;
                println!("  #{i:02}  {ms:>5} ms  FAIL: {e}");
            }
        }
    }

    latencies.sort_unstable();
    let ok = latencies.len();
    println!("\nresults ({ok} ok / {failures} fail):");
    if ok > 0 {
        let sum: u128 = latencies.iter().sum();
        println!(
            "  min {} ms · p50 {} ms · p90 {} ms · p99 {} ms · max {} ms · mean {} ms",
            latencies[0],
            percentile(&latencies, 50.0),
            percentile(&latencies, 90.0),
            percentile(&latencies, 99.0),
            latencies[ok - 1],
            sum / ok as u128
        );
    }
    if let Some(out) = last_out {
        println!("  last outAmount: {out}");
    }

    Ok(())
}
