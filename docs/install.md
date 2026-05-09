# Install

## Prereqs

- macOS (launchd-managed daemon)
- For source build: `rustc` ≥ 1.95 (`brew install rust`)

## Quick install (from source)

```bash
make install         # build + ccft install
ccft trust --apply   # write env into ~/.claude.json (with backup)
```

`ccft install` does five things, idempotently:

1. Generates a self-signed CA at `~/.cc-flytrap/{ca.pem,ca.key}` (if missing).
2. Writes a default config at `~/.config/ccft/ccft.json` (if missing).
3. Copies the running binary to `~/.local/bin/ccft`.
4. Writes `~/Library/LaunchAgents/com.ccft.plist` pointing at the installed binary, with `RunAtLoad` and `KeepAlive`.
5. `launchctl bootstrap`s the plist into the user domain.

After install, the flytrap is running on `127.0.0.1:7178`. To route Claude through it:

```bash
ccft trust --apply   # writes HTTPS_PROXY + NODE_EXTRA_CA_CERTS into ~/.claude.json (with backup)
# — or, manually —
export HTTPS_PROXY=http://127.0.0.1:7178
export NODE_EXTRA_CA_CERTS=$HOME/.cc-flytrap/ca.pem
```

`ccft trust --revoke` reverses the env edits cleanly.

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

`ccft dev` runs the same flytrap in foreground with isolated state:

| | Production (`ccft run`) | Dev (`ccft dev`) |
|---|---|---|
| Port | 7178 | 7179 |
| Config | `~/.config/ccft/ccft.json` | `~/.config/ccft/dev.json` |
| Ledger | `~/.local/share/ccft/ledger.jsonl` | `~/.local/share/ccft/dev/ledger.jsonl` |
| Process | launchd-managed | foreground, dies with the shell |
| CA | shared `~/.cc-flytrap/ca.pem` | shared `~/.cc-flytrap/ca.pem` |

To use dev: `HTTPS_PROXY=http://127.0.0.1:7179 NODE_EXTRA_CA_CERTS=$HOME/.cc-flytrap/ca.pem claude -p "..."`. The CA is shared so trust setup carries over.
