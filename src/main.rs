use std::time::Instant;

// Day 1 of a Rust port of the quote-collection module from my Python
// solana-tca-monitor: one Jupiter quote (100 USDC -> wSOL), measure the
// request latency, print the essentials.

const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const WSOL: &str = "So11111111111111111111111111111111111111112";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 100 USDC in atomic units (USDC has 6 decimals): 100 * 10^6
    let amount: u64 = 100_000_000;

    let url = format!(
        "https://lite-api.jup.ag/swap/v1/quote?inputMint={USDC}&outputMint={WSOL}&amount={amount}&slippageBps=50"
    );

    let t0 = Instant::now();
    let resp = reqwest::blocking::get(&url)?; // `?` — if this fails, return the error (like `raise`)
    let latency_ms = t0.elapsed().as_millis();

    let status = resp.status();
    let body: serde_json::Value = resp.json()?; // dynamic JSON, like a Python dict for now

    println!("status     : {status}");
    println!("latency    : {latency_ms} ms");
    println!("outAmount  : {}", body["outAmount"]);
    println!(
        "route hops : {}",
        body["routePlan"].as_array().map(|a| a.len()).unwrap_or(0)
    );

    Ok(())
}
