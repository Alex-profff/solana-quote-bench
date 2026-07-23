# solana-quote-bench

A Rust port — in progress — of the quote-collection module from my Python
[solana-tca-monitor](https://github.com/Alex-profff/solana-tca-monitor):
fetch Jupiter quotes for a fixed basket, measure quote latency (p50/p99),
and derive implied prices. Same logic, new language.

Why: my production tooling is Python; this port is my hands-on path into Rust,
done the way I actually work — a real module, preserved behavior, verified output.

## Status

- [x] Day 1 — toolchain, first Jupiter quote request + latency measurement
- [x] Day 2 — quote loop + latency percentiles (p50/p99) + Python-vs-Rust bench
      (below). Also fixed Day-1 semantics: timing now wraps the **full body**
      like the Python original — Day 1 measured only time-to-headers.
- [ ] Typed response structs (serde), route-plan parsing
- [ ] Token basket + CSV output, CLI args (clap)

## Python vs Rust — same measurement, same wire

30 quotes each, back-to-back runs on the same machine, same endpoint
(Jupiter lite-api), same request ($100 USDC → wSOL). Timing wraps full body
download + JSON parse. Python side: [`bench/bench_python.py`](bench/bench_python.py)
(`requests.Session`); Rust side: this binary (`reqwest` blocking `Client`) —
both reuse the connection.

| ms     | min | p50 | p90 | p99 | max | mean | ok/fail |
|--------|-----|-----|-----|-----|-----|------|---------|
| Python | 151 | 154 | 161 | 914 | 914 | 180  | 30/0    |
| Rust   | 160 | 162 | 164 | 951 | 951 | 188  | 30/0    |

p99 = the first request of each run (TLS handshake) — at n=30, p99 is
literally that one call.

**Honest read: no language win here — quote latency is network-bound.** The
point of the port is measurement parity (the Rust version reproduces the
Python collector's semantics, verified on the same wire), not a speed claim.
Day 1 accidentally measured something different — catching *why the two
versions disagreed* is the exercise.

## Run

```
cargo run -- 30                  # 30 quotes, prints percentiles
python bench/bench_python.py 30  # same measurement, Python side
```
