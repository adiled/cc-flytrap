#!/usr/bin/env python3
"""
ccft perf — observability layer for ccft itself.

Answers the question: "is ccft slowing my requests down?"

Decomposes each request's wall-time into three buckets, drawn from the ledger:

    wall    total time the flow spent under ccft's control
    upstream  streaming response duration (api.anthropic.com → us)
    pre     wall - upstream  (waiting for first byte: api TTFT + our work)
    ccft    measured internal processing time (json parse, modify, hooks)

Verdict logic compares median ccft time to median wall time. If ccft is
under a small absolute and relative threshold, it's not the bottleneck.

Pure stdlib. Reads ~/.local/share/ccft/ledger.jsonl (and archive/).
"""

import json
import os
import sys
import time
from datetime import datetime
from pathlib import Path

LEDGER = Path(os.environ.get(
    'CCFT_LEDGER',
    str(Path.home() / '.local' / 'share' / 'ccft' / 'ledger.jsonl')
))
ARCHIVE = LEDGER.parent / 'archive'

NO_COLOR = bool(os.environ.get('NO_COLOR')) or not sys.stdout.isatty()


def c(code, s):
    return s if NO_COLOR else f"\x1b[{code}m{s}\x1b[0m"


def dim(s):     return c("2", s)
def bold(s):    return c("1", s)
def red(s):     return c("31", s)
def green(s):   return c("32", s)
def yellow(s):  return c("33", s)
def grey(s):    return c("90", s)


def parse_range(spec):
    now = time.time()
    today_start = datetime.now().replace(
        hour=0, minute=0, second=0, microsecond=0
    ).timestamp()
    spec = (spec or '').strip().lower()
    if spec in ('', 'today'):
        return today_start, now, 'today'
    if spec == 'yesterday':
        return today_start - 86400, today_start, 'yesterday'
    if spec in ('week', '7d'):
        return now - 7 * 86400, now, 'last 7d'
    if spec == '24h':
        return now - 86400, now, 'last 24h'
    if spec.endswith('h'):
        try:
            n = int(spec[:-1])
            return now - n * 3600, now, f'last {n}h'
        except ValueError:
            pass
    if spec.endswith('d'):
        try:
            n = int(spec[:-1])
            return now - n * 86400, now, f'last {n}d'
        except ValueError:
            pass
    if spec == 'all':
        return 0, now, 'all-time'
    raise SystemExit(
        f"perf: don't understand range '{spec}'. "
        "Try: today, yesterday, 24h, 7d, all, Nh, Nd"
    )


def iter_records(since, until):
    files = []
    if ARCHIVE.exists():
        files.extend(sorted(ARCHIVE.glob('ledger_*.jsonl')))
    if LEDGER.exists():
        files.append(LEDGER)
    for f in files:
        try:
            with open(f) as fh:
                for line in fh:
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        r = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    ts = r.get('ts', 0)
                    if ts < since or ts > until:
                        continue
                    yield r
        except FileNotFoundError:
            continue


def percentile(values, p):
    if not values:
        return 0
    s = sorted(values)
    k = (len(s) - 1) * p / 100
    f = int(k)
    c2 = min(f + 1, len(s) - 1)
    return s[f] + (s[c2] - s[f]) * (k - f)


def fmt_ms(ms):
    if ms < 1:
        return f"{ms*1000:.0f}us"
    if ms < 1000:
        return f"{int(ms)}ms"
    return f"{ms/1000:.1f}s"


def fmt_us(us):
    if us < 1000:
        return f"{int(us)}us"
    if us < 1_000_000:
        return f"{us/1000:.1f}ms"
    return f"{us/1_000_000:.1f}s"


def verdict(ccft_p50_us, wall_p50_ms, ccft_coverage_pct):
    """Plain-language read on whether ccft is the bottleneck.

    Verdict is based on the apples-to-apples ratio (ccft vs wall on the same
    records), so we don't need to hold off until coverage is high. We do warn
    when the sample is very small.
    """
    ccft_ms = ccft_p50_us / 1000
    rel = (ccft_ms / wall_p50_ms * 100) if wall_p50_ms > 0 else 0
    sample_warn = ''
    if ccft_coverage_pct < 5:
        sample_warn = dim(f"  (small sample — {ccft_coverage_pct:.0f}% of records)")

    if ccft_ms < 5 and rel < 1:
        return ('clean',
                green(f"ccft contributes ~{rel:.2f}% of wall time. "
                      "not the bottleneck — slowness is upstream.") + sample_warn)
    if ccft_ms < 30 and rel < 3:
        return ('small',
                green(f"ccft adds ~{ccft_ms:.1f}ms median ({rel:.1f}%). "
                      "small, well within network noise.") + sample_warn)
    if ccft_ms < 100 and rel < 10:
        return ('measurable',
                yellow(f"ccft adds ~{ccft_ms:.0f}ms median ({rel:.0f}%). "
                       "measurable but probably acceptable.") + sample_warn)
    return ('investigate',
            red(f"⚠ ccft adds ~{ccft_ms:.0f}ms median ({rel:.0f}% of wall). "
                "worth investigating.") + sample_warn)


def cmd_show(args):
    spec = ' '.join(args) if args else 'today'
    since, until, label = parse_range(spec)

    walls = []          # ms — total time per request (all records)
    upstreams = []      # ms — streaming response duration
    pres = []           # ms — wait-for-first-byte
    ccfts = []          # us — ccft internal processing
    walls_with_ccft = []  # ms — wall time of records that ALSO have ccft timing
                          # (used for the apples-to-apples verdict ratio)
    n_total = 0

    for r in iter_records(since, until):
        n_total += 1
        ts = r.get('ts', 0)
        te = r.get('te', 0)
        lat = r.get('lat', 0) or 0
        wall_ms = (te - ts) * 1000
        if wall_ms <= 0:
            continue
        walls.append(wall_ms)
        upstreams.append(lat)
        pres.append(max(0, wall_ms - lat))
        c_us = r.get('c_us')
        if c_us is not None and c_us > 0:
            ccfts.append(c_us)
            walls_with_ccft.append(wall_ms)

    n_with_ccft = len(ccfts)

    print()
    print(bold(f"  ccft perf · {label}"))
    print(grey("  ───────────────────────────────────────────────────────────"))

    if n_total == 0:
        print(dim("  (no records in range)\n"))
        return

    def show_row(name, values, formatter, color=None):
        p50 = percentile(values, 50)
        p95 = percentile(values, 95)
        p99 = percentile(values, 99)
        col = color or (lambda s: s)
        print(f"  {name:10}  p50 {col(formatter(p50)):>9}  "
              f"p95 {col(formatter(p95)):>9}  "
              f"p99 {col(formatter(p99)):>9}")

    show_row('wall',     walls,     fmt_ms)
    show_row('upstream', upstreams, fmt_ms, color=dim)
    show_row('pre',      pres,      fmt_ms, color=dim)
    if ccfts:
        show_row('ccft',     ccfts,     fmt_us)
    else:
        print(f"  {'ccft':10}  {dim('no records with ccft timing yet — run more traffic')}")

    coverage_pct = (n_with_ccft / n_total * 100) if n_total else 0
    print()
    print(f"  {bold('records')}    {n_total}  "
          f"{dim('·')} {n_with_ccft} with ccft timing  "
          f"{dim('·')} wall = upstream + pre")

    if ccfts:
        ccft_p50 = percentile(ccfts, 50)
        # apples-to-apples: wall p50 of the same records that have ccft timing
        wall_p50 = percentile(walls_with_ccft, 50)
        kind, msg = verdict(ccft_p50, wall_p50, coverage_pct)
        print()
        print(f"  {bold('verdict')}    {msg}")
    print()


def cmd_help(args):
    print("""\
usage: ccft perf [today|yesterday|24h|7d|Nh|Nd|all]

  Reports ccft's contribution to request latency, derived from the ledger.

  wall      total time the flow spent under ccft (request received → response complete)
  upstream  streaming response duration (api.anthropic.com → us)
  pre       wall - upstream — wait for first byte (mostly API TTFT)
  ccft      measured ccft internal processing time

  The verdict at the bottom answers "is ccft slowing me down?".
""")


def main(argv):
    if not argv or argv[0] in ('show',):
        return cmd_show(argv[1:] if argv else [])
    if argv[0] in ('help', '--help', '-h'):
        return cmd_help(argv[1:])
    return cmd_show(argv)


if __name__ == '__main__':
    try:
        main(sys.argv[1:])
    except BrokenPipeError:
        pass
    except KeyboardInterrupt:
        print()
