# cc-flytrap

Claude Code optimizer. ~95% less tokens → faster + cheaper.

## Install

Give your LLM this prompt:

```
Install cc-flytrap from https://github.com/adiled/cc-flytrap
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

## Config

`~/.config/ccft/ccft.json`:

```json
{"port": 7178, "host": "127.0.0.1", "system_override": ""}
```

## Logs

`~/.local/share/ccft/logs/`