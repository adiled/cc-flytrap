# cc-flytrap

Claude Code System Prompt Stripper - Intercepts Claude Code's API calls and strips bloated system prompts (~95% reduction).

## Quick Install

```bash
cd ~/cc-flytrap
./bin/ccft install
```

## Usage

Claude automatically uses flytrap if configured. Otherwise:

```bash
HTTP_PROXY=http://127.0.0.1:7178 \
HTTPS_PROXY=http://127.0.0.1:7178 \
NODE_EXTRA_CA_CERTS=~/.mitmproxy/mitmproxy-ca-cert.pem \
claude -p "your prompt"
```

## Commands

```bash
ccft start      # Start flytrap
ccft stop       # Stop flytrap
ccft status     # Check status
ccft test       # Run smoke test
ccft install    # Install as service
ccft uninstall  # Remove service
ccft ledger    # Show/reset/archive usage
ccft update     # Update to latest
```

## Requirements

1. **mitmproxy** - Install with `brew install mitmproxy` (macOS) or `sudo apt install mitmproxy` (Linux)
2. **CA Certificate** - Run `mitmproxy` once, then install certificate from http://mitm.it to your system keychain

## Configuration

Edit `~/.config/ccft/ccft.json`:

```json
{
  "system_override": "",
  "port": 3128,
  "host": "127.0.0.1"
}
```

- `system_override` - Custom text to inject (optional)
- `port` - Flytrap listener port (default: 7178)
- `host` - Flytrap listener host (default: 127.0.0.1)

## What it does

Original: ~4,000 words per request  
Stripped: ~200 words per request (~95% reduction)

- Block 1: Billing header (kept)
- Block 2: Identity prefix (kept - modifying triggers 500 errors)
- Block 3: Trimmed with system_override tag
- Block 4: Trimmed

## Ledger

Usage records stored in JSONL format at `~/.local/share/ccft/ledger.jsonl`.

```bash
ccft ledger show      # Show last 5 records
ccft ledger records  # Show last 20
ccft ledger reset    # Archive and reset
```

## Logs

```bash
tail -f ~/.local/share/ccft/logs/launchd.log   # macOS
tail -f ~/.local/share/ccft/logs/systemd.log  # Linux
```