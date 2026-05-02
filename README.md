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

## Brainrot

Two-axis vibe-check on your sessions. Splits **bot drift** (output collapse, latency choke) from **driver drift** (input bloat, rapid-fire, session sprawl). Tells you whose fault.

```
bot      37/100  — fine
driver   71/100  — cooked 🔥
         ↳ prompts are bloating or driver is rapid-firing
```

Behavioural only — derived from velocities, latencies, permutations, and volumetrics in the ledger. Never reads your content.

| Command | What |
|---|---|
| `ccft brainrot` | Today's dashboard |
| `ccft brainrot week` | 7-day rollup |
| `ccft brainrot replay --follow` | Live tail of new requests |
| `ccft brainrot diff today yesterday` | Compare two ranges |
| `ccft brainrot session [sid]` | Drill into one session |
| `ccft brainrot score` | One-liner for status bars |

## Config

`~/.config/ccft/ccft.json`:

```json
{"port": 7178, "host": "127.0.0.1", "system_override": ""}
```

## Logs

`~/.local/share/ccft/logs/`
