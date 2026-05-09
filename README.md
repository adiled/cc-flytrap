# ccft

<img width="1412" height="774" alt="ccft TUI" src="https://github.com/user-attachments/assets/f34de8ae-ba83-4326-b7e8-06d03ca1f0bd" />

**ccft — an agentic self improvement tool.** A streaming flytrap that sits between Claude Code and `api.anthropic.com`, mutates the request system prompt to your preferences, and writes a per-response token ledger — all while preserving Claude's token-by-token streaming UX byte-for-byte.

Three design properties:

1. **One artifact, one location.** The binary at `~/.local/bin/ccft` is everything. No install dir, no rsync.
2. **Streaming is preserved.** A custom `Body` wrapper taps the upstream SSE chunk-by-chunk. Tokens reach the client at the same cadence as direct.
3. **Tiny resident footprint.** ~6 MB on disk, ~4 MB resident at idle.

> Service auto-start runs on **macOS** (launchd) and **Linux** (systemd-user). On Windows, `ccft install` sets up the binary + CA + config; auto-start isn't wired yet — run `ccft run` manually.

## Install

Prebuilt binary from the latest release:

```bash
# macOS (universal aarch64 + x86_64)
curl -L https://github.com/adiled/ccft/releases/latest/download/ccft-macos-universal -o /usr/local/bin/ccft

# Linux (x86_64)
curl -L https://github.com/adiled/ccft/releases/latest/download/ccft-linux-x86_64 -o /usr/local/bin/ccft

chmod +x /usr/local/bin/ccft
ccft install
ccft trust --apply
```

Or build from source (`brew install rust`):

```bash
make install
ccft trust --apply
```

`ccft install` provisions the CA, default config, plist, and launchd unit. `ccft trust --apply` writes `HTTPS_PROXY` + `NODE_EXTRA_CA_CERTS` into `~/.claude.json` (with backup). Full lifecycle in [`docs/install.md`](docs/install.md).

## What's inside

**TUI** — `ccft` at a tty opens a full-screen dashboard: brainrot chart (bot/driver vibes over time), heat-by-time bars, recent-traffic ledger, sessions/perf overlays. Range dial keys: `t y h w W a`.

**Ledger** — every request gets a JSONL line at `~/.local/share/ccft/ledger.jsonl` with input/output tokens, cache hits, latency, model, session id, and ccft's own processing time. Schema in [`docs/reference.md`](docs/reference.md#ledger-schema).

**Dev mode** — `ccft dev` runs in foreground on port 7179 with an isolated config + ledger. No need to bootout the production daemon. Details in [`docs/install.md#dev-mode`](docs/install.md#dev-mode).

**Config** — three knobs in `~/.config/ccft/ccft.json`: `system_override` (extra system prompt), `pain` (false trims Claude Code's bloat blocks), `ledger` (write JSONL). See [`docs/reference.md#config`](docs/reference.md#config).

**Architecture** — hudsucker (hyper-1.x + tokio + rustls), host-gated to `api.anthropic.com` only. Other CONNECT requests pass straight through, so `gh`, `git`, `npm`, `pip` keep working from any subprocess. See [`docs/architecture.md`](docs/architecture.md).

## Docs

- [Install / uninstall / lifecycle / dev mode](docs/install.md)
- [Config / ledger schema / file layout / CLI reference](docs/reference.md)
- [How it works / TUI / dependencies](docs/architecture.md)

## License

MIT — see [`LICENSE`](LICENSE).
