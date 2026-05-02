# cc-flytrap

Claude Code System Prompt Stripper - Intercepts Claude Code's API calls and strips bloated system prompts (~95% reduction).

## Quick Install

```bash
cd ~/cc-flytrap
./install.sh
```

## Usage

Run Claude through the proxy:

```bash
HTTP_PROXY=http://127.0.0.1:3128 \
HTTPS_PROXY=http://127.0.0.1:3128 \
NODE_EXTRA_CA_CERTS=~/.mitmproxy/mitmproxy-ca-cert.pem \
claude -p "your prompt"
```

Or with clwnd - add to `~/.config/clwnd/clwnd.json`:

```json
{
  "ccFlags": {
    "HTTP_PROXY": "http://127.0.0.1:3128",
    "HTTPS_PROXY": "http://127.0.0.1:3128",
    "NODE_EXTRA_CA_CERTS": "~/.mitmproxy/mitmproxy-ca-cert.pem"
  }
}
```

## Commands

```bash
./install.sh        # Install and start service
./install.sh --status   # Check service status
./install.sh --uninstall   # Stop and remove service
```

## Requirements

1. **mitmproxy** - Install with `brew install mitmproxy` (macOS) or `sudo apt install mitmproxy` (Linux)
2. **CA Certificate** - Run `mitmproxy` once, then install certificate from http://mitm.it to your system keychain

## What it does

Original: ~4,000 words per request  
Stripped: ~200 words per request (~95% reduction)

- Block 1: Billing header (kept)
- Block 2: Identity prefix (kept - modifying triggers 500 errors)
- Block 3: Trimmed with system_override tag
- Block 4: Trimmed

## Logs

```bash
tail -f ~/cc-flytrap/logs/launchd.log   # macOS
tail -f ~/cc-flytrap/logs/systemd.log  # Linux
```