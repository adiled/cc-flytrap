# cc-flytrap

Claude Code optimizer. ~95% less tokens → faster + cheaper.

## Install

```bash
mkdir -p ~/.local/share/ccft
curl -sL https://github.com/adiled/cc-flytrap/releases/latest/download/cc-flytrap.tar.gz | tar -xz -C ~/.local/share/ccft
~/.local/share/ccft/bin/ccft install
```

## Use

Claude uses it automatically after install. Or:

```bash
HTTP_PROXY=127.0.0.1:7178 HTTPS_PROXY=127.0.0.1:7178 \
  NODE_EXTRA_CA_CERTS=~/.mitmproxy/mitmproxy-ca-cert.pem claude -p "hi"
```

## Commands

| Command | Description |
|---------|-------------|
| `ccft start` | Start optimizer |
| `ccft stop` | Stop |
| `ccft status` | Check running |
| `ccft install` | Install as service |
| `ccft update` | Update |

## Config

`~/.config/ccft/ccft.json`:

```json
{"port": 7178, "host": "127.0.0.1", "system_override": ""}
```

## Logs

`~/.local/share/ccft/logs/`