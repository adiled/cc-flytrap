# ccft

<img width="1412" height="774" alt="Screenshot 2026-05-09 at 4 18 39 PM" src="https://github.com/user-attachments/assets/f34de8ae-ba83-4326-b7e8-06d03ca1f0bd" />


**ccft - an agentic self improvement tool.** A streaming MITM proxy that sits between Claude Code and `api.anthropic.com`, mutates the request system prompt to your preferences, and writes a per-response token ledger — all while preserving Claude's token-by-token streaming UX byte-for-byte.

Three concrete design properties:

1. **One artifact, one location.** The binary at `~/.local/bin/ccft` is everything. No install dir, no rsync between source and install, no plist hardcoded to a vendored copy.
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

Each line in `~/.local/share/ccft/ledger.jsonl`:

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

The TUI brainrot panel reads this file as-is.

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

**Keyboard:**

| Key | What |
|---|---|
| `←` / `→` (or `[` / `]`) | step through range presets |
| `t` `y` `h` `w` `W` `a` | jump to today / yday / 24h / 7d / this-week / all |
| `r` | force refresh |
| `d` `s` `p` `l` | overlay: split / sessions / perf / live |
| `?` | help overlay |
| `Esc` | close overlay |
| `q` / `Ctrl-C` | quit |

The header **status block** (right side) is always-on: port-bound dot, pid, uptime, clock. There's no separate "status pane" — the proxy health is permanently in the chrome.

The **range and zoom dials** at the bottom are the primary interactivity. Time is the X axis; everything else (overlays, filters, models pane) keeps the same range so drilling preserves context.

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
| `ccft tui` (or `ccft` alone at a tty) | Full-screen interactive dashboard with time-dimension dials |

## Sources / dependencies

- [hudsucker](https://github.com/omjadas/hudsucker) — MIT/Apache, hyper-based MITM proxy library
- [rcgen](https://github.com/rustls/rcgen) — CA + cert generation
- [rustls](https://github.com/rustls/rustls) + `aws-lc-rs` — TLS server side
- [serde_json](https://github.com/serde-rs/json) — JSON parse + emit for request mutation, ledger, config
- [clap](https://github.com/clap-rs/clap) — CLI args + subcommands
- [dashmap](https://github.com/xacrimon/dashmap) — concurrent map for the request→response flow stash
- [ratatui](https://ratatui.rs/) — TUI rendering
- [tokio](https://tokio.rs/), [hyper](https://hyper.rs/) — async runtime + HTTP plumbing

## Earlier pilot (lua/)

`lua/` contains a pre-Rust pilot that ran cc-flytrap as a Lua plugin inside the [proxelar](https://github.com/emanuele-em/proxelar) MITM proxy. It worked for request mutation but proxelar buffers response bodies whenever a script is loaded — the streaming UX collapses to one chunk at end-of-stream. This Rust implementation was the response to that limit.
