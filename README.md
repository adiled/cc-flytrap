# cc-flytrap

Claude Code Optimizer - Makes Claude Code faster and cheaper by removing API bloat.

**Result**: ~95% less tokens per request → lower latency, lower cost.

## Quick Install

```bash
cd ~/cc-flytrap
./bin/ccft install
```

## Usage

Claude automatically uses flytrap after install. Or manually:

```bash
HTTP_PROXY=http://127.0.0.1:7178 \
HTTPS_PROXY=http://127.0.0.1:7178 \
NODE_EXTRA_CA_CERTS=~/.mitmproxy/mitmproxy-ca-cert.pem \
claude -p "your prompt"
```

## Commands

```bash
ccft start      # Start optimizer
ccft stop       # Stop optimizer
ccft status     # Check status
ccft test       # Run smoke test
ccft install    # Install as service
ccft uninstall  # Remove service
ccft ledger    # Show/reset usage records
ccft update     # Update to latest
```

## Requirements

1. **mitmproxy** - `brew install mitmproxy` (macOS) or `sudo apt install mitmproxy` (Linux)
2. **CA Certificate** - Run `mitmproxy` once, then install from http://mitm.it

## Configuration

`~/.config/ccft/ccft.json`:

```json
{
  "system_override": "",
  "port": 7178,
  "host": "127.0.0.1"
}
```

- `system_override` - Custom instruction text (optional)
- `port` - Listener port (default: 7178)
- `host` - Listener host (default: 127.0.0.1)

## Ledger

Usage records at `~/.local/share/ccft/ledger.jsonl`:

```bash
ccft ledger show      # Last 5 records
ccft ledger records  # Last 20
ccft ledger reset    # Archive and reset
```

## Logs

```bash
tail -f ~/.local/share/ccft/logs/launchd.log   # macOS
tail -f ~/.local/share/ccft/logs/systemd.log  # Linux
```