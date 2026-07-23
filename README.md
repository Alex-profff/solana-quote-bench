# solana-quote-bench

A Rust port — in progress — of the quote-collection module from my Python
[solana-tca-monitor](https://github.com/Alex-profff/solana-tca-monitor):
fetch Jupiter quotes for a fixed basket, measure quote latency (p50/p99),
and derive implied prices. Same logic, new language.

Why: my production tooling is Python; this port is my hands-on path into Rust,
done the way I actually work — a real module, preserved behavior, verified output.

## Status

- [x] Day 1 — toolchain, first Jupiter quote request + latency measurement
- [ ] Quote loop over a basket, typed response structs (serde)
- [ ] Latency percentiles (p50/p99), CSV output
- [ ] CLI args (clap), results in README

## Run

```
cargo run
```
