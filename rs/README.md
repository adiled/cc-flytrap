# ccft

**ccft - an agentic self improvement tool.** A streaming MITM proxy that sits between Claude Code and `api.anthropic.com`, mutates the request system prompt to your preferences, and writes a per-response token ledger — all while preserving Claude's token-by-token streaming UX byte-for-byte.

This is the Rust pilot. It supersedes the Python `cc-flytrap.py` + `bin/ccft*` bash scripts in `../`, with three concrete improvements:

1. **One artifact, one location.** The binary at `~/.local/bin/ccft` is everything. No `~/.local/share/ccft/` install dir, no rsync between source and install, no plist hardcoded to a vendored copy.
2. **Streaming is preserved.** A custom `Body` wrapper taps the upstream SSE stream chunk-by-chunk, parsing usage tokens as bytes pass through. Tokens reach the client at the same cadence as direct (~300-450 ms apart).
3. **Tiny resident footprint.** ~6 MB on disk, ~4 MB resident at idle.

## Install

Prereq: `rustc` ≥ 1.95 (`brew install rust`).

```bash
make install         # build + ccft install
ccft trust           # print the env vars Claude needs
```

`ccft install` does five things, idempotently:

1. Generates a self-signed CA at `~/.cc-flytrap/{ca.pem,ca.key}` (if missing).
2. Writes a default config at `~/.config/ccft/ccft.json` (if missing).
3. Copies the running binary to `~/.local/bin/ccft`.
4. Writes `~/Library/LaunchAgents/com.ccft.plist` pointing at the installed binary, with `RunAtLoad` and `KeepAlive`.
5. `launchctl bootstrap`s the plist into the user domain.

After install, the proxy is running on `127.0.0.1:7178`. To route Claude through it:

```bash
ccft trust --apply   # writes HTTPS_PROXY + NODE_EXTRA_CA_CERTS into ~/.claude.json (with backup)
# — or, manually —
export HTTPS_PROXY=http://127.0.0.1:7178
export NODE_EXTRA_CA_CERTS=$HOME/.cc-flytrap/ca.pem
```

`--apply` always backs up `~/.claude.json` to `.claude.json.bak` first. `ccft trust --revoke` removes the keys.

## Uninstall

```bash
ccft uninstall
```

Bootout, removes the plist, removes the installed binary. **Keeps** the CA cert, config, and ledger so a re-install picks up where you left off. To purge:

```bash
rm -rf ~/.cc-flytrap ~/.config/ccft ~/.local/share/ccft
```

## Lifecycle

```bash
ccft status                  # is it loaded? bound? on which port?
ccft start                   # kick launchd
ccft stop                    # bootout (will respawn on next login)
ccft restart                 # bootout + bootstrap
ccft logs                    # tail launchd output
ccft logs -n 200             # last 200 lines
```

## Dev mode

The thing the bash version got wrong: a separate `ccft-dev` script that ran a different mitmdump invocation pointing at vendored Python paths. It worked but every dev change required an `rsync` + a `ccft restart`. With a single binary, the dev story collapses:

```bash
make dev                     # builds, then runs `ccft dev` in foreground
# — or, hot iterate —
cargo run --release -- dev
```

`ccft dev` runs the same proxy in foreground with isolated state:

| | Production (`ccft run`) | Dev (`ccft dev`) |
|---|---|---|
| Port | 7178 | 7179 |
| Config | `~/.config/ccft/ccft.json` | `~/.config/ccft/dev.json` |
| Ledger | `~/.local/share/ccft/ledger.jsonl` | `~/.local/share/ccft/dev/ledger.jsonl` |
| Process | launchd-managed | foreground, dies with the shell |
| CA | shared `~/.cc-flytrap/ca.pem` | shared `~/.cc-flytrap/ca.pem` |

To use dev: `HTTPS_PROXY=http://127.0.0.1:7179 NODE_EXTRA_CA_CERTS=$HOME/.cc-flytrap/ca.pem claude -p "..."`. The CA is shared so trust setup carries over.

No rsync. No "production drift from source" problem. The build artifact is the dev artifact is the install artifact.

## Config

`~/.config/ccft/ccft.json` (or `$CCFT_CONFIG`):

```json
{
  "host":            "127.0.0.1",
  "port":            7178,
  "system_override": "",
  "pain":            false,
  "ledger":          true
}
```

| Key | Default | Meaning |
|---|---|---|
| `host` | `"127.0.0.1"` | Bind address |
| `port` | `7178` | Bind port |
| `system_override` | `""` | Extra system block injected into every `/v1/messages`. Empty = skip injection. |
| `pain` | `false` | `false` trims Claude Code's three large bloat blocks; `true` leaves them alone. |
| `ledger` | `true` | Write per-response JSONL records to `~/.local/share/ccft/ledger.jsonl`. |

Config reload requires restart (`ccft restart`).

## Ledger schema

Schema-compatible with `cc-flytrap/ledger.py`. Each line in `~/.local/share/ccft/ledger.jsonl`:

```json
{
  "ts": 1778104532.67977, "te": 1778104534.415335,
  "dt": "2026-05-06 21:55:32",
  "human": "adil", "agent": "host-70bbe330",
  "sid": "170bf0f2-ae59-46b6-af51-22c1b107c08e",
  "cip": "127.0.0.1:54188", "pip": "124.29.237.112", "sip": null,
  "ep": "https://api.anthropic.com/v1/messages?beta=true",
  "reg": null, "model": "claude-haiku-4-5-20251001",
  "in": 520, "out": 84, "tot": 604, "lat": 758,
  "cr": 0, "cc": 0, "c_us": 115
}
```

Brainrot reads this file as-is.

## How it works

```
claude  ──HTTPS_PROXY=http://127.0.0.1:7178──>  ccft  ──HTTPS h1.1──>  api.anthropic.com
                                                  │
                                                  ├─ on_request:  decode → mutate `system` array → re-encode → forward
                                                  │
                                                  └─ on_response: wrap Body with SseTap →
                                                                  every chunk forwarded to client + parsed for SSE usage events →
                                                                  on stream end, append ledger.jsonl line
```

Built on [`hudsucker`](https://github.com/omjadas/hudsucker), a hyper-1.x + tokio + rustls MITM proxy library. h1.1 is forced (Anthropic accepts it cleanly via ALPN, which sidesteps the open h2 issues across the Go/Rust MITM ecosystem).

## TUI

`ccft` with no subcommand at a tty opens the full-screen interactive dashboard. Brainrot is the frame; everything else is a knob or an overlay.

```
▍ CCFT // 0.0.1   PROXY ONLINE                        port:7178  pid:39983  up:2h  04:11:32

┏ PROXY ━━━━━━━━┓ ┏ BRAINROT PANEL ━━━━━━━━━━━━━━━━━━━━━━━━━━━━ ┓ ┏ LEDGER ━━━━━━━━━━━━━━┓
┃ ◉ 7178        ┃ ┃ BOT vs DRIVER · today    today yday 24h 7d  ┃ ┃ TIME    Δin   Δout    ┃
┃ ◉ ledger      ┃ ┃                                              ┃ ┃ 22:51   +12   +7      ┃
┃ ⌁ 24 reqs     ┃ ┃ <bot/driver line chart>                      ┃ ┃ 22:46   +520  +84     ┃
┃ ⌖ 1 session   ┃ ┃                                              ┃ ┃ ...                   ┃
┣ MODELS ━━━━━━━┫ ┃ BOT  DRIVER  P99 LAT  CACHE                  ┃ ┗━━━━━━━━━━━━━━━━━━━━━━┛
┃ ◉ opus-4.7 95%┃ ┃ 37    39      131ms     82%                  ┃
┃ ◉ haiku-4.5 5%┃ ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
┣ HEAT ━━━━━━━━━┫ ┏ STREAM ━━━━━━━━━━━━┓ ┏ DIAGNOSIS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃ 00··▁▂▃······ ┃ ┃ 22:51 in:12 out:7 ┃ ┃ vibe:   bot fine · driver fine                   ┃
┗━━━━━━━━━━━━━━━┛ ┗━━━━━━━━━━━━━━━━━━━┛ ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
[q]uit │ [r]efresh │ [/]filter │ [t]oday [y]day [w]eek [a]ll │ [d]split [s]essions [p]erf [l]ive │ ←/→ range
```

**Keyboard:**

| Key | What |
|---|---|
| `←` / `→` (or `[` / `]`) | step through range presets |
| `t` `y` `w` `h` `a` | jump to today / yday / 7d / 24h / all |
| `r` | force refresh |
| `d` `s` `p` `l` | overlay: split / sessions / perf / live |
| `?` | help overlay |
| `Esc` | close overlay |
| `q` / `Ctrl-C` | quit |

The header **status block** (right side) is always-on: port-bound dot, pid, uptime, clock. There's no separate "status pane" — the proxy health is permanently in the chrome.

The **range and zoom dials** at the bottom are the primary interactivity. Time is the X axis; everything else (overlays, filters, models pane) keeps the same range so drilling preserves context.

## brainrot + perf

`ccft brainrot` and `ccft perf` are read-only consumers of `~/.local/share/ccft/ledger.jsonl`. They render the ledger into terminal dashboards with cyberpunk ccft chrome (header banner, scanline rule, magenta section markers, neon cyan accents).

```
▍ CCFT ▸ brainrot · today
▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔

▶ vibe
  bot      37 / 100   — fine
  driver   39 / 100   — fine
```

**brainrot** subcommands (all derive metrics from V·L·P·V telemetry — never inspect content):

| Subcommand | What |
|---|---|
| `today` (default) | Daily dashboard — bot/driver scores, traffic, latency p50/p99, burn sparkline, by-hour heatmap, models, streak |
| `week` | 7-day rollup — daily bars, day-of-week pattern, peaks |
| `score [range]` | One-line bot/driver score (good for status bars) |
| `split [range]` | Driver vs bot turn split — classification by inter-arrival gap |
| `session [sid]` | List today's sessions, or drill into one by sid prefix |
| `replay [range] [--speed N] [--follow]` | Animated playback; `--follow` tails the live ledger |
| `diff A B` | Compare two ranges side by side with drift % |

**perf** answers "is ccft slowing my requests down?":

```
wall      total time the flow spent under ccft
upstream  api.anthropic.com → us streaming duration
pre       wall − upstream                            (TTFB + ccft pre-work)
ccft      measured internal processing time
```

Verdict compares median ccft to median wall (apples-to-apples on records that have `c_us`):

```
▶ verdict
  ◆ ccft contributes ~0.04% of wall time. not the bottleneck — slowness is upstream.
```

## File layout

| Path | Owner | What |
|---|---|---|
| `~/.local/bin/ccft` | `ccft install` | The binary itself |
| `~/Library/LaunchAgents/com.ccft.plist` | `ccft install` | launchd unit |
| `~/.cc-flytrap/ca.pem`, `ca.key` | `ccft install` (or first run) | Self-signed CA |
| `~/.config/ccft/ccft.json` | user / `ccft install` (default) | Production config |
| `~/.config/ccft/dev.json` | user (optional) | Dev config |
| `~/.local/share/ccft/ledger.jsonl` | runtime | Production ledger |
| `~/.local/share/ccft/state.jsonl` | runtime | `ledger_on`/`ledger_off` events |
| `~/.local/share/ccft/dev/ledger.jsonl` | `ccft dev` | Dev ledger |
| `~/.local/share/ccft/logs/launchd.log` | launchd | stdout+stderr from the service |

## Subcommand reference

| Command | What |
|---|---|
| `ccft run` | Run proxy in foreground using production config (what launchd invokes) |
| `ccft dev` | Run proxy in foreground using dev config (port 7179, isolated ledger) |
| `ccft install` | Copy binary, generate CA, write plist, bootstrap launchd |
| `ccft uninstall` | Bootout, remove plist + binary, keep CA/config/ledger |
| `ccft status` | Print install + load + bind state |
| `ccft start` | `launchctl kickstart` |
| `ccft stop` | `launchctl bootout` |
| `ccft restart` | bootout + bootstrap |
| `ccft trust` | Print env vars for Claude |
| `ccft trust --apply` | Write env into `~/.claude.json` (with backup) |
| `ccft trust --revoke` | Remove env from `~/.claude.json` |
| `ccft trust --ca` | Dump CA PEM to stdout |
| `ccft logs [-n 50]` | Tail `launchd.log` |
| `ccft brainrot [today\|week\|score\|split\|session\|replay\|diff]` | Time-series vibe analyzer over the ledger |
| `ccft perf [today\|24h\|7d\|...]` | Decompose request wall time; verdict on whether ccft is the bottleneck |
| `ccft tui` (or `ccft` alone at a tty) | Full-screen brainrot dashboard with time-dimension dials |

## Why this exists / what it replaces

The Python cc-flytrap predecessor in `../` had three operational pain points the user kept running into:

1. **Two install dirs** (source repo + `~/.local/share/ccft/`) kept in sync by a manual `rsync`. Every dev change → rsync → `ccft restart`.
2. **The launchd plist hardcoded vendored paths** into the install dir. Updating those paths required editing the plist or reinstalling.
3. **The bash installer mutated `~/.claude.json`** to add `HTTP_PROXY` / `HTTPS_PROXY` / `NODE_EXTRA_CA_CERTS`. When the proxy went down, claude got connection-refused — the user had to remove env from `claude.json` to recover.

This implementation:

1. **Has no install dir.** The binary is the artifact. No rsync, no source-vs-install distinction. Dev mode runs from `target/release/ccft` directly.
2. **The plist points at one absolute path** (`~/.local/bin/ccft`). Updating ccft = `make install` overwrites the binary; launchd auto-restarts.
3. **`ccft trust` is opt-in and explicit.** `ccft install` does NOT touch `~/.claude.json`. You run `ccft trust --apply` if you want it persisted, otherwise `export` ad-hoc. `--revoke` walks it back cleanly. Backup is automatic.

## Sources / dependencies

- [hudsucker](https://github.com/omjadas/hudsucker) — MIT/Apache, hyper-based MITM proxy library
- [rcgen](https://github.com/rustls/rcgen) — CA + cert generation
- [rustls](https://github.com/rustls/rustls) + `aws-lc-rs` — TLS server side
- [serde_json](https://github.com/serde-rs/json) — JSON parse + emit for request mutation, ledger, config
- [clap](https://github.com/clap-rs/clap) — CLI args + subcommands
- [dashmap](https://github.com/xacrimon/dashmap) — concurrent map for the request→response flow stash
- [tokio](https://tokio.rs/), [hyper](https://hyper.rs/) — async runtime + HTTP plumbing

## Earlier pilot (lua/)

`../lua/` contains a pre-Rust pilot that ran cc-flytrap as a Lua plugin inside the [proxelar](https://github.com/emanuele-em/proxelar) MITM proxy. It worked for request mutation but proxelar buffers response bodies whenever a script is loaded — the streaming UX collapses to one chunk at end-of-stream. The investigation is documented in `../lua/README.md`. This Rust implementation was the response to that limit.
