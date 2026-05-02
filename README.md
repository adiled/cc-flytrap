# cc-flytrap

Claude Code optimizer. ~95% less tokens → faster + cheaper.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/adiled/cc-flytrap/main/install.sh | bash
```

## Use

After install, Claude uses it automatically. Restart Claude if already running.

## Commands

| Command | Description |
|---------|-------------|
| `ccft start` | Start optimizer |
| `ccft stop` | Stop |
| `ccft status` | Check running |
| `ccft install` | Install as service |
| `ccft update` | Update |
| `ccft brainrot` | Vibe-check your sessions (see below) |
| `ccft perf` | Is ccft adding latency? (see below) |

## Brainrot

Two-axis vibe-check on your sessions. Splits **bot drift** (output collapse, latency choke) from **driver drift** (input bloat, rapid-fire, session sprawl). Tells you whose fault.

<img width="752" height="486" alt="Screenshot 2026-05-02 at 6 04 06 PM" src="https://github.com/user-attachments/assets/1f160ebb-fd17-4874-a580-cca06a28e5ef" />

Behavioural only — derived from velocities, latencies, permutations, and volumetrics in the ledger. Never reads your content.

| Command | What |
|---|---|
| `ccft brainrot` | Today's dashboard |
| `ccft brainrot week` | 7-day rollup |
| `ccft brainrot replay --follow` | Live tail of new requests |
| `ccft brainrot diff today yesterday` | Compare two ranges |
| `ccft brainrot session [sid]` | Drill into one session |
| `ccft brainrot score` | One-liner for status bars |

## Perf

ccft's own observability layer. Answers: *is ccft slowing my requests down?*

```
ccft perf · today
─────────────────
wall        p50  1.2s   p95  8.4s   p99  31s
upstream    p50  760ms  p95  7.1s   p99  26s
pre         p50  480ms  p95  1.2s   p99  5.4s
ccft        p50  2ms    p95  8ms    p99  14ms

verdict     ccft contributes ~0.2% of wall time. not the bottleneck —
            slowness is upstream.
```

| Bucket | Meaning |
|---|---|
| `wall` | total time the flow spent under ccft (request received → response complete) |
| `upstream` | streaming response duration (`api.anthropic.com` → us) |
| `pre` | `wall − upstream` — wait for first byte (mostly API TTFT) |
| `ccft` | measured ccft internal processing (json parse, modify, hooks) |

The verdict gives a plain-language answer based on the median ccft time, both in absolute milliseconds and as a percentage of wall time.

## Config

`~/.config/ccft/ccft.json`:

```json
{
  "port": 7178,
  "host": "127.0.0.1",
  "system_override": "",
  "pain": false,
  "ledger": true
}
```

| Key | Default | Meaning |
|---|---|---|
| `pain` | `false` | `false` = trim bloated system prompts. `true` = leave them alone (passive observer). |
| `ledger` | `true` | Record per-request telemetry to `~/.local/share/ccft/ledger.jsonl`. Set `false` to disable. |
| `system_override` | `""` | Custom system prompt to inject. Empty = use built-in default. Injected regardless of `pain`. |

## Logs

`~/.local/share/ccft/logs/`
