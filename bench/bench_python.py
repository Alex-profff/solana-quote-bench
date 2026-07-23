"""Python side of the latency bench — the measurement the Rust binary ports.

Same semantics on purpose: requests.Session (keep-alive, like reqwest's
blocking Client), timing wraps the request incl. full body download + JSON
parse, same endpoint, same size. Run back-to-back with the Rust binary on
the same machine for a fair comparison.
"""
import sys
import time

import requests

USDC = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
WSOL = "So11111111111111111111111111111111111111112"
URL = (
    f"https://lite-api.jup.ag/swap/v1/quote?inputMint={USDC}"
    f"&outputMint={WSOL}&amount=100000000&slippageBps=50"
)


def percentile(sorted_vals, p):
    if not sorted_vals:
        return 0
    idx = round((len(sorted_vals) - 1) * p / 100)
    return sorted_vals[idx]


def main(n=30):
    print(f"bench_python: {n} Jupiter quotes, full-body latency")
    lat, failures = [], 0
    s = requests.Session()
    for i in range(1, n + 1):
        t0 = time.perf_counter()
        try:
            r = s.get(URL, timeout=10)
            r.raise_for_status()
            r.json()
            ms = round((time.perf_counter() - t0) * 1000)
            lat.append(ms)
            print(f"  #{i:02d}  {ms:>5} ms  ok")
        except Exception as e:  # noqa: BLE001 — bench tool, count and move on
            ms = round((time.perf_counter() - t0) * 1000)
            failures += 1
            print(f"  #{i:02d}  {ms:>5} ms  FAIL: {e}")

    lat.sort()
    print(f"\nresults ({len(lat)} ok / {failures} fail):")
    if lat:
        print(
            f"  min {lat[0]} ms · p50 {percentile(lat, 50)} ms"
            f" · p90 {percentile(lat, 90)} ms · p99 {percentile(lat, 99)} ms"
            f" · max {lat[-1]} ms · mean {round(sum(lat) / len(lat))} ms"
        )


if __name__ == "__main__":
    main(int(sys.argv[1]) if len(sys.argv) > 1 else 30)
