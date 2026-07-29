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
- [x] Day 3 — typed deserialization (serde structs, no more dynamic JSON),
      implied price, route-plan parsing and route churn (findings below)
- [x] Day 4 — optional CSV export of each quote (latency, implied price, route)
      so a run can be inspected after the fact
- [x] Day 5 — unit tests for the percentile function (empty / single / p0-p50-p100)
- [x] Day 6 — known-answer test for `implied_price` (100 USDC → 1 SOL ⇒ $100/SOL)
- [x] Day 7 — measure a **basket** (wSOL / JUP / BONK): per-token latency,
      implied price, price impact and route churn (finding below)
- [ ] CLI args (clap), configurable basket / notional

## Day 3 finding: the route churns under identical requests

20 back-to-back quotes for the *same* request ($100 USDC → wSOL, seconds apart)
returned **5 distinct routes**:

```
 12x  HumidiFi (100%)
  3x  SolFi V2 (100%)
  3x  Quantum (100%)
  1x  Scorch (100%)
  1x  Quantum (100%) → Scorch (100%)
```

Implied price stayed tight across all of them — mean **$75.8028/SOL**, range
$75.8015–$75.8082, spread **0.009%** — so the venue changed far more than the
price did. That is the point of tracking route churn separately from price:
*the fill you priced is not necessarily the fill you get*, even when the number
looks stable. The Python monitor tracks the same metric; this port reproduces it.

Amounts arrive as JSON **strings**, not numbers — a `u64` lamport amount can
exceed JavaScript's safe integer range, so the API quotes them to keep full
precision. Parsing them explicitly (rather than letting them land in a float)
is the correctness detail a dynamic-JSON version hides.

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

## Day 7 finding: the cost lives in the token, not the number

Same $100 USDC notional, 12 quotes each, back-to-back:

| token | mean price impact | implied-price spread | typical route |
|-------|-------------------|----------------------|---------------|
| wSOL  | 0.0034%           | 0.006%               | single-hop (Quantum / BisonFi / HumidiFi) |
| JUP   | 0.34%             | 0.002%               | ZeroFi → Whirlpool (2 hops) |
| BONK  | 0.41%             | 0.007%               | ZeroFi → Whirlpool (2 hops) |

For the *same trade size* the quoted price impact is ~100x larger on the
midcaps than on wSOL — and the majors fill in a single hop while both midcaps
route through a second venue (Whirlpool). Latency is basically flat across all
three (~145 ms p50), i.e. still network-bound, not token-bound.

That is the whole point of measuring a basket rather than one pair: a single
liquid pair reports a cost near zero and hides where execution actually gets
expensive. The Python monitor tracks the basket for exactly this reason; the
port now does too.

## Run

```
cargo run -- 20                  # 20 quotes per token, per-token summary
cargo run -- 20 out.csv          # ...and write every quote (with token) to out.csv
python bench/bench_python.py 30  # single-pair (wSOL) Python latency bench, same wire
```
