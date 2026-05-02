#!/usr/bin/env python3
"""
ccft brainrot - Time-series vibe analyzer for the ledger.

Turns the ledger's audit trail into addictive, scrollable insights.
Pure stdlib. Streams JSONL, never loads whole archive into memory.

Subcommands:
    today       Today's dashboard (default)
    week        7-day rollup
    replay      Animated playback of recent activity
    diff A B    Compare two time ranges
    session     Per-session drill-in (lists if no sid)
    score       One-line brainrot score
"""

import json
import os
import sys
import time
from collections import defaultdict, Counter
from datetime import datetime
from pathlib import Path

# ─── Paths ────────────────────────────────────────────────────────────────────

LEDGER = Path(os.environ.get(
    'CCFT_LEDGER',
    str(Path.home() / '.local' / 'share' / 'ccft' / 'ledger.jsonl')
))
ARCHIVE = LEDGER.parent / 'archive'

# ─── ANSI ─────────────────────────────────────────────────────────────────────

NO_COLOR = bool(os.environ.get('NO_COLOR')) or not sys.stdout.isatty()


def c(code, s):
    if NO_COLOR:
        return s
    return f"\x1b[{code}m{s}\x1b[0m"


def dim(s):     return c("2", s)
def bold(s):    return c("1", s)
def red(s):     return c("31", s)
def green(s):   return c("32", s)
def yellow(s):  return c("33", s)
def blue(s):    return c("34", s)
def magenta(s): return c("35", s)
def cyan(s):    return c("36", s)
def grey(s):    return c("90", s)


SPARK = "▁▂▃▄▅▆▇█"
HEAT_THRESHOLDS = [
    (0,    grey),
    (500,  green),
    (1500, cyan),
    (3000, yellow),
    (6000, red),
]


def heat_color(latency_ms):
    fn = grey
    for thresh, color in HEAT_THRESHOLDS:
        if latency_ms >= thresh:
            fn = color
    return fn


def sparkline(values, width=None):
    if not values:
        return ""
    if width and len(values) > width:
        bucket = len(values) / width
        vs = []
        for i in range(width):
            chunk = values[int(i*bucket):int((i+1)*bucket)] or [0]
            vs.append(sum(chunk) / len(chunk))
        values = vs
    mx = max(values) or 1
    return "".join(SPARK[min(7, int(v / mx * 7))] for v in values)


# ─── Ledger streaming ─────────────────────────────────────────────────────────

def iter_records(since=None, until=None):
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
                    if since is not None and ts < since:
                        continue
                    if until is not None and ts > until:
                        continue
                    yield r
        except FileNotFoundError:
            continue


# ─── Time parsing ─────────────────────────────────────────────────────────────

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
    if spec in ('prev 7d', 'prev-7d', 'prev_week'):
        return now - 14 * 86400, now - 7 * 86400, 'prev 7d'
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
    try:
        d = datetime.fromisoformat(spec)
        return d.timestamp(), d.timestamp() + 86400, spec
    except ValueError:
        pass

    raise SystemExit(
        f"brainrot: don't understand range '{spec}'. "
        "Try: today, yesterday, 24h, 7d, prev 7d, all, or YYYY-MM-DD"
    )


# ─── Aggregation ──────────────────────────────────────────────────────────────

def aggregate(records):
    a = {
        'n': 0,
        'in': 0, 'out': 0, 'tot': 0,
        'lat_sum': 0, 'lat_max': 0, 'lats': [],
        'first_ts': None, 'last_ts': None,
        'models': Counter(),
        'sessions': set(),
        'humans': set(),
        'agents': set(),
        'by_hour': defaultdict(lambda: {'n': 0, 'tot': 0, 'lat_sum': 0, 'models': Counter()}),
        'by_day': defaultdict(lambda: {'n': 0, 'tot': 0, 'in': 0, 'out': 0, 'lat_sum': 0}),
        'by_minute': defaultdict(lambda: {'tot': 0, 'n': 0}),
        'records': [],
    }
    for r in records:
        a['n'] += 1
        a['in'] += r.get('in', 0)
        a['out'] += r.get('out', 0)
        a['tot'] += r.get('tot', 0)
        lat = r.get('lat', 0) or 0
        a['lat_sum'] += lat
        a['lat_max'] = max(a['lat_max'], lat)
        a['lats'].append(lat)
        ts = r.get('ts', 0)
        a['first_ts'] = ts if a['first_ts'] is None else min(a['first_ts'], ts)
        a['last_ts'] = ts if a['last_ts'] is None else max(a['last_ts'], ts)
        a['models'][r.get('model', 'unknown')] += 1
        if r.get('sid'):
            a['sessions'].add(r['sid'])
        if r.get('human'):
            a['humans'].add(r['human'])
        if r.get('agent'):
            a['agents'].add(r['agent'])

        d = datetime.fromtimestamp(ts)
        hb = a['by_hour'][d.hour]
        hb['n'] += 1
        hb['tot'] += r.get('tot', 0)
        hb['lat_sum'] += lat
        hb['models'][r.get('model', 'unknown')] += 1

        day = d.strftime('%Y-%m-%d')
        db = a['by_day'][day]
        db['n'] += 1
        db['tot'] += r.get('tot', 0)
        db['in'] += r.get('in', 0)
        db['out'] += r.get('out', 0)
        db['lat_sum'] += lat

        mb = a['by_minute'][int(ts // 60)]
        mb['n'] += 1
        mb['tot'] += r.get('tot', 0)

        a['records'].append(r)
    return a


def percentile(values, p):
    if not values:
        return 0
    s = sorted(values)
    k = (len(s) - 1) * p / 100
    f = int(k)
    c2 = min(f + 1, len(s) - 1)
    return int(s[f] + (s[c2] - s[f]) * (k - f))


def fmt_n(n):
    if n >= 1_000_000:
        return f"{n/1_000_000:.1f}M"
    if n >= 1_000:
        return f"{n/1_000:.1f}k"
    return str(int(n))


def fmt_dur(seconds):
    if seconds < 60:
        return f"{int(seconds)}s"
    if seconds < 3600:
        return f"{int(seconds/60)}m"
    if seconds < 86400:
        h = int(seconds / 3600)
        m = int((seconds % 3600) / 60)
        return f"{h}h{m}m"
    return f"{int(seconds/86400)}d"


def short_model(m):
    if not m:
        return 'unknown'
    parts = m.replace('claude-', '').split('-')
    if not parts:
        return m
    name = parts[0]
    ver = ''
    if len(parts) >= 3 and parts[1].isdigit():
        ver = f"-{parts[1]}.{parts[2]}" if parts[2].isdigit() else f"-{parts[1]}"
    elif len(parts) >= 2 and parts[1].isdigit():
        ver = f"-{parts[1]}"
    return f"{name}{ver}"


# ─── Brainrot score ───────────────────────────────────────────────────────────

def brainrot_score(a):
    if a['n'] == 0:
        return 0, 'idle'
    avg_lat = a['lat_sum'] / a['n']
    avg_in = a['in'] / a['n']
    p99 = percentile(a['lats'], 99)

    lat_score = min(33, avg_lat / 100)
    bloat_score = min(33, avg_in / 1500)
    p99_score = min(34, p99 / 200)
    score = int(lat_score + bloat_score + p99_score)

    if score < 20:
        vibe = 'crisp 🧊'
    elif score < 40:
        vibe = 'fine'
    elif score < 60:
        vibe = 'mid'
    elif score < 80:
        vibe = 'cooked 🔥'
    else:
        vibe = 'fried 💀'
    return score, vibe


# ─── Subcommand: today / range ────────────────────────────────────────────────

def cmd_today(args):
    spec = ' '.join(args) if args else 'today'
    since, until, label = parse_range(spec)
    a = aggregate(iter_records(since, until))

    print()
    print(bold(f"  brainrot · {label}  "))
    print(grey("  ───────────────────────────────────────────────"))

    if a['n'] == 0:
        print(dim("  (no records — go make some API calls)\n"))
        return

    score, vibe = brainrot_score(a)
    score_color = green if score < 40 else yellow if score < 70 else red
    print(f"  {bold('score')}    {score_color(str(score))}/100  {dim('—')} {vibe}")

    avg_lat = a['lat_sum'] / a['n']
    p50 = percentile(a['lats'], 50)
    p99 = percentile(a['lats'], 99)
    span = (a['last_ts'] - a['first_ts']) if a['first_ts'] else 0
    sess_count = len(a['sessions'])
    sess_word = 'session' if sess_count == 1 else 'sessions'

    print(f"  {bold('reqs')}     {a['n']}  "
          f"{dim('over')} {fmt_dur(span)}  "
          f"{dim('·')} {sess_count} {sess_word}")
    print(f"  {bold('tokens')}   {fmt_n(a['in'])} in  {dim('·')}  "
          f"{fmt_n(a['out'])} out  "
          f"{dim('·')}  {fmt_n(a['tot'])} total")
    p99_str = f"{p99}ms"
    print(f"  {bold('latency')}  p50 {p50}ms  {dim('·')}  "
          f"p99 {heat_color(p99)(p99_str)}  "
          f"{dim('·')}  avg {int(avg_lat)}ms")

    if a['by_minute']:
        m_keys = sorted(a['by_minute'].keys())
        first_min = m_keys[0]
        last_min = m_keys[-1]
        width = min(60, max(10, last_min - first_min + 1))
        series = []
        for m in range(first_min, last_min + 1):
            series.append(a['by_minute'].get(m, {'tot': 0})['tot'])
        spark = sparkline(series, width=width)
        peak_idx = series.index(max(series)) if series else 0
        peak_min = first_min + peak_idx
        peak_dt = datetime.fromtimestamp(peak_min * 60)
        print()
        print(f"  {bold('burn')}     {cyan(spark)}")
        print(f"           {dim('peak')} {peak_dt.strftime('%H:%M')} "
              f"{dim('·')} {fmt_n(max(series))} tok/min")

    if a['by_hour']:
        print()
        print(f"  {bold('by hour')}  {grey('00         06         12         18         23')}")
        line = "           "
        for h in range(24):
            hb = a['by_hour'].get(h)
            if not hb or hb['n'] == 0:
                line += dim('·')
            else:
                hp50 = hb['lat_sum'] / hb['n']
                intensity = min(7, int(hb['n'] / max(1, a['n']/24) * 4))
                ch = SPARK[intensity]
                line += heat_color(hp50)(ch)
        print(line)

    if a['models']:
        print()
        print(f"  {bold('models')}")
        total = sum(a['models'].values())
        bar_w = 40
        for model, count in a['models'].most_common(5):
            pct = count / total
            filled = int(pct * bar_w)
            bar = ('█' * filled) + ('░' * (bar_w - filled))
            label_m = short_model(model)
            pct_str = dim(f"{int(pct*100)}%")
            print(f"    {label_m:14} {cyan(bar)} {pct_str} ({count})")

    now = time.time()
    recent_30 = sum(1 for r in a['records'] if r.get('ts', 0) > now - 1800)
    if recent_30:
        print()
        print(f"  {bold('streak')}   🔥 {recent_30} reqs in last 30min")
    elif a['last_ts'] and now - a['last_ts'] < 86400:
        idle = now - a['last_ts']
        print()
        print(f"  {bold('streak')}   💤 idle {fmt_dur(idle)}")

    if a['by_hour']:
        peak_hour = max(a['by_hour'].items(), key=lambda kv: kv[1]['n'])
        slow_hour = max(
            a['by_hour'].items(),
            key=lambda kv: kv[1]['lat_sum'] / max(1, kv[1]['n'])
        )
        if peak_hour[0] == slow_hour[0]:
            print()
            warn = (f"  ⚠  your peak hour ({peak_hour[0]:02}:00) is also "
                    f"your slowest. you're choking the model.")
            print(dim(warn))

    print()


# ─── Subcommand: replay ───────────────────────────────────────────────────────

def cmd_replay(args):
    speed = 1.0
    range_spec = '24h'
    follow = False
    rest = []
    i = 0
    while i < len(args):
        if args[i] in ('--speed', '-s') and i + 1 < len(args):
            speed = float(args[i + 1])
            i += 2
        elif args[i] in ('--follow', '-f'):
            follow = True
            i += 1
        else:
            rest.append(args[i])
            i += 1
    if rest:
        range_spec = ' '.join(rest)

    since, until, label = parse_range(range_spec)
    records = list(iter_records(since, until))
    if not records and not follow:
        print(dim(f"\n  no records in {label}\n"))
        return

    print()
    print(bold(f"  brainrot replay · {label} · {len(records)} records · {speed}x"))
    print(grey("  ───────────────────────────────────────────────────────────────"))

    prev_ts = records[0].get('ts', 0) if records else time.time()
    for r in records:
        ts = r.get('ts', 0)
        gap = max(0, ts - prev_ts)
        sleep_s = min(2.0, gap / speed) if speed > 0 else 0
        if sleep_s > 0.01:
            time.sleep(sleep_s)
        prev_ts = ts

        when = datetime.fromtimestamp(ts).strftime('%H:%M:%S')
        model = short_model(r.get('model', '?'))
        in_t = fmt_n(r.get('in', 0))
        out_t = fmt_n(r.get('out', 0))
        lat = r.get('lat', 0) or 0
        sid = (r.get('sid') or '')[:8]
        marker = '⚠ slow' if lat > 5000 else '· fast' if lat < 500 else ''
        model_tag = f"[{model:11}]"
        lat_str = f"{lat}ms"
        print(f"  {dim(when)}  {cyan(model_tag)}  "
              f"in:{in_t:>6}  out:{out_t:>6}  "
              f"lat:{heat_color(lat)(lat_str):>8}  "
              f"{dim('sid:'+sid)}  {yellow(marker)}")

    if follow:
        print(grey("\n  following live ledger… (ctrl-c to stop)\n"))
        try:
            last_size = LEDGER.stat().st_size if LEDGER.exists() else 0
            while True:
                time.sleep(0.5)
                if not LEDGER.exists():
                    continue
                size = LEDGER.stat().st_size
                if size > last_size:
                    with open(LEDGER) as fh:
                        fh.seek(last_size)
                        for line in fh:
                            line = line.strip()
                            if not line:
                                continue
                            try:
                                r = json.loads(line)
                            except json.JSONDecodeError:
                                continue
                            ts = r.get('ts', 0)
                            when = datetime.fromtimestamp(ts).strftime('%H:%M:%S')
                            lat = r.get('lat', 0) or 0
                            model = short_model(r.get('model', '?'))
                            model_tag = f"[{model:11}]"
                            lat_str = f"{lat}ms"
                            print(f"  {green('●')} {dim(when)}  "
                                  f"{cyan(model_tag)}  "
                                  f"in:{fmt_n(r.get('in',0)):>6}  "
                                  f"out:{fmt_n(r.get('out',0)):>6}  "
                                  f"lat:{heat_color(lat)(lat_str)}")
                    last_size = size
        except KeyboardInterrupt:
            print(dim("\n  bye\n"))


# ─── Subcommand: diff ─────────────────────────────────────────────────────────

def cmd_diff(args):
    if len(args) < 2:
        ranges = ['today', 'yesterday']
    else:
        ranges = args[:2]

    since_a, until_a, label_a = parse_range(ranges[0])
    since_b, until_b, label_b = parse_range(ranges[1])

    a = aggregate(iter_records(since_a, until_a))
    b = aggregate(iter_records(since_b, until_b))

    print()
    print(bold(f"  brainrot diff · {label_a}  vs  {label_b}"))
    print(grey("  ───────────────────────────────────────────────────────────────"))

    if a['n'] == 0 and b['n'] == 0:
        print(dim("  (no records in either period)\n"))
        return

    def delta(av, bv, lower_is_better=True):
        if bv == 0:
            return ('—', grey)
        pct = (av - bv) / bv * 100
        sign = '+' if pct >= 0 else ''
        good = (pct < 0) if lower_is_better else (pct > 0)
        col = green if good else (red if abs(pct) > 5 else yellow)
        return (f"{sign}{pct:.0f}%", col)

    score_a, _ = brainrot_score(a)
    score_b, _ = brainrot_score(b)
    avg_lat_a = (a['lat_sum'] / a['n']) if a['n'] else 0
    avg_lat_b = (b['lat_sum'] / b['n']) if b['n'] else 0
    avg_in_a = (a['in'] / a['n']) if a['n'] else 0
    avg_in_b = (b['in'] / b['n']) if b['n'] else 0
    p99_a = percentile(a['lats'], 99)
    p99_b = percentile(b['lats'], 99)
    ratio_a = (a['out'] / a['in']) if a['in'] else 0
    ratio_b = (b['out'] / b['in']) if b['in'] else 0

    rows = [
        ('score',     f"{score_a}/100",     f"{score_b}/100",      delta(score_a, score_b, True)),
        ('requests',  str(a['n']),          str(b['n']),           delta(a['n'], b['n'], False)),
        ('total tok', fmt_n(a['tot']),      fmt_n(b['tot']),       delta(a['tot'], b['tot'], False)),
        ('avg in',    fmt_n(avg_in_a),      fmt_n(avg_in_b),       delta(avg_in_a, avg_in_b, True)),
        ('avg lat',   f"{int(avg_lat_a)}ms", f"{int(avg_lat_b)}ms", delta(avg_lat_a, avg_lat_b, True)),
        ('p99 lat',   f"{p99_a}ms",         f"{p99_b}ms",          delta(p99_a, p99_b, True)),
        ('out/in',    f"{ratio_a:.2f}",     f"{ratio_b:.2f}",      delta(ratio_a, ratio_b, False)),
        ('sessions',  str(len(a['sessions'])), str(len(b['sessions'])),
                                                                   delta(len(a['sessions']), len(b['sessions']), False)),
    ]

    print(f"  {'metric':12} {label_a:>14}  {label_b:>14}    drift")
    print(grey("  ───────────────────────────────────────────────────────────────"))
    for label, va, vb, (d, col) in rows:
        print(f"  {label:12} {va:>14}  {vb:>14}    {col(d)}")

    print()
    print(f"  {bold('model mix shift')}")
    all_models = set(a['models']) | set(b['models'])
    tot_a = sum(a['models'].values()) or 1
    tot_b = sum(b['models'].values()) or 1
    for m in sorted(all_models):
        pa = a['models'].get(m, 0) / tot_a * 100
        pb = b['models'].get(m, 0) / tot_b * 100
        diff_pp = pa - pb
        sign = '+' if diff_pp >= 0 else ''
        col = green if abs(diff_pp) < 5 else (yellow if abs(diff_pp) < 15 else red)
        diff_str = f"{sign}{diff_pp:.1f}pp"
        print(f"    {short_model(m):14}  {pa:5.1f}%  vs  {pb:5.1f}%   {col(diff_str)}")

    print()


# ─── Subcommand: session ──────────────────────────────────────────────────────

def cmd_session(args):
    if not args:
        since, until, _ = parse_range('today')
        sessions = defaultdict(lambda: {
            'n': 0, 'tot': 0, 'first': None, 'last': None,
            'lat_sum': 0, 'models': Counter()
        })
        for r in iter_records(since, until):
            sid = r.get('sid') or '(no-sid)'
            s = sessions[sid]
            s['n'] += 1
            s['tot'] += r.get('tot', 0)
            s['lat_sum'] += r.get('lat', 0) or 0
            ts = r.get('ts', 0)
            s['first'] = ts if s['first'] is None else min(s['first'], ts)
            s['last'] = ts if s['last'] is None else max(s['last'], ts)
            s['models'][r.get('model', '?')] += 1

        if not sessions:
            print(dim("\n  no sessions today\n"))
            return

        print()
        print(bold(f"  sessions · today  ({len(sessions)})"))
        print(grey("  ───────────────────────────────────────────────────────────────"))
        print(f"  {'sid':10}  {'reqs':>5}  {'tokens':>8}  {'avg lat':>9}  {'span':>7}  model")
        for sid, s in sorted(sessions.items(), key=lambda kv: -kv[1]['n']):
            sid_short = sid[:8] if sid != '(no-sid)' else sid
            avg_lat = int(s['lat_sum'] / s['n']) if s['n'] else 0
            span = fmt_dur((s['last'] or 0) - (s['first'] or 0))
            top_model = (
                short_model(s['models'].most_common(1)[0][0]) if s['models'] else '?'
            )
            lat_str = f"{avg_lat}ms"
            print(f"  {sid_short:10}  {s['n']:>5}  {fmt_n(s['tot']):>8}  "
                  f"{heat_color(avg_lat)(lat_str):>9}  {span:>7}  {dim(top_model)}")
        print()
        return

    sid_query = args[0]
    matched = [r for r in iter_records(0, time.time())
               if (r.get('sid') or '').startswith(sid_query)]
    if not matched:
        print(dim(f"\n  no session matching '{sid_query}'\n"))
        return

    full_sid = matched[0].get('sid')
    a = aggregate(matched)
    span = a['last_ts'] - a['first_ts']
    api_time = a['lat_sum'] / 1000
    pct_in_api = api_time / max(1, span) * 100

    print()
    print(bold(f"  session {full_sid}"))
    print(grey("  ───────────────────────────────────────────────────────────────"))
    print(f"  {bold('reqs')}      {a['n']}")
    print(f"  {bold('span')}      {fmt_dur(span)} wall  "
          f"{dim('·')}  {fmt_dur(api_time)} in API  "
          f"{dim(f'({pct_in_api:.0f}%)')}")
    print(f"  {bold('tokens')}    {fmt_n(a['in'])} in  {dim('·')}  {fmt_n(a['out'])} out")
    print(f"  {bold('model')}     {short_model(a['models'].most_common(1)[0][0])}")
    print()
    print(grey("  timeline:"))
    for i, r in enumerate(a['records']):
        when = datetime.fromtimestamp(r.get('ts', 0)).strftime('%H:%M:%S')
        gap_str = ''
        if i > 0:
            g = r.get('ts', 0) - a['records'][i-1].get('ts', 0)
            if g > 30:
                gap_str = dim(f"  +{fmt_dur(g)} thinking")
        lat = r.get('lat', 0) or 0
        lat_str = f"{lat}ms"
        print(f"  {dim(when)}  in:{fmt_n(r.get('in',0)):>6}  "
              f"out:{fmt_n(r.get('out',0)):>6}  "
              f"lat:{heat_color(lat)(lat_str)}{gap_str}")
    print()


# ─── Subcommand: week ─────────────────────────────────────────────────────────

def cmd_week(args):
    since, until, label = parse_range('7d')
    a = aggregate(iter_records(since, until))

    print()
    print(bold(f"  brainrot · {label}"))
    print(grey("  ───────────────────────────────────────────────────────────────"))

    if a['n'] == 0:
        print(dim("  (no records this week)\n"))
        return

    score, vibe = brainrot_score(a)
    print(f"  {bold('score')}    {score}/100  {dim('—')} {vibe}")
    print(f"  {bold('reqs')}     {a['n']}  {dim('·')}  "
          f"{fmt_n(a['tot'])} tokens  {dim('·')}  "
          f"{len(a['sessions'])} sessions")

    print()
    print(f"  {bold('daily')}")
    days = sorted(a['by_day'].items())
    max_tot = max((d['tot'] for _, d in days), default=1)
    for day, d in days:
        dt = datetime.fromisoformat(day)
        dow = dt.strftime('%a')
        bar_w = 40
        filled = int(d['tot'] / max_tot * bar_w)
        bar = '█' * filled + '░' * (bar_w - filled)
        avg_lat = int(d['lat_sum'] / d['n']) if d['n'] else 0
        lat_str = f"{avg_lat}ms"
        nreq = dim(f"{d['n']} req")
        print(f"    {dow} {dt.strftime('%m-%d')}  {cyan(bar)}  "
              f"{fmt_n(d['tot']):>7}  {nreq}  "
              f"{heat_color(avg_lat)(lat_str)}")

    dow_count = defaultdict(int)
    for day, d in days:
        dt = datetime.fromisoformat(day)
        dow_count[dt.weekday()] += d['n']
    print()
    print(f"  {bold('day pattern')}")
    dow_labels = 'MTWTFSS'
    max_dow = max(dow_count.values()) or 1
    line = "    "
    for i, lbl in enumerate(dow_labels):
        intensity = min(7, int(dow_count.get(i, 0) / max_dow * 7))
        line += f"{lbl}{SPARK[intensity]} "
    print(line)

    if a['by_hour']:
        peak_hour = max(a['by_hour'].items(), key=lambda kv: kv[1]['n'])
        worst_hour = max(
            a['by_hour'].items(),
            key=lambda kv: kv[1]['lat_sum'] / max(1, kv[1]['n'])
        )
        print()
        print(f"  {bold('peaks')}")
        print(f"    busiest hour:  {peak_hour[0]:02}:00  ({peak_hour[1]['n']} reqs)")
        worst_p50 = worst_hour[1]['lat_sum'] / max(1, worst_hour[1]['n'])
        worst_str = f"{int(worst_p50)}ms avg"
        print(f"    slowest hour:  {worst_hour[0]:02}:00  "
              f"({heat_color(worst_p50)(worst_str)})")

    print()
    print(f"  {bold('models')}")
    total = sum(a['models'].values())
    bar_w = 40
    for model, count in a['models'].most_common(5):
        pct = count / total
        filled = int(pct * bar_w)
        bar = '█' * filled + '░' * (bar_w - filled)
        pct_str = dim(f"{int(pct*100)}%")
        print(f"    {short_model(model):14} {cyan(bar)} {pct_str} ({count})")

    print()


# ─── Subcommand: score ────────────────────────────────────────────────────────

def cmd_score(args):
    spec = ' '.join(args) if args else 'today'
    since, until, label = parse_range(spec)
    a = aggregate(iter_records(since, until))
    score, vibe = brainrot_score(a)
    summary = f"({a['n']} reqs, {fmt_n(a['tot'])} tok)"
    if NO_COLOR:
        print(f"brainrot {score}/100 {vibe} {summary}")
    else:
        col = green if score < 40 else (yellow if score < 70 else red)
        print(f"brainrot {col(str(score))}/100 {vibe} {dim(summary)}")


# ─── Dispatch ─────────────────────────────────────────────────────────────────

USAGE = """\
usage: ccft brainrot [subcommand] [args]

  (no args)        today's vibe dashboard
  today [range]    dashboard for a range
  week             7-day rollup
  replay [range]   animated playback  [--speed N]  [--follow]
  diff A B         compare two ranges
  session [sid]    list sessions today, or drill into one
  score [range]    one-line brainrot score (good for status bars)

ranges:  today, yesterday, 24h, 7d, prev 7d, all, YYYY-MM-DD, Nh, Nd
"""


def main(argv):
    if not argv:
        return cmd_today([])
    sub = argv[0]
    rest = argv[1:]
    handlers = {
        'today':   cmd_today,
        'week':    cmd_week,
        'replay':  cmd_replay,
        'diff':    cmd_diff,
        'session': cmd_session,
        'score':   cmd_score,
        'help':    lambda _: print(USAGE),
        '--help':  lambda _: print(USAGE),
        '-h':      lambda _: print(USAGE),
    }
    if sub in handlers:
        return handlers[sub](rest)
    return cmd_today(argv)


if __name__ == '__main__':
    try:
        main(sys.argv[1:])
    except BrokenPipeError:
        pass
    except KeyboardInterrupt:
        print()
