# cc-flytrap → proxelar pilot

A pilot investigation into replacing cc-flytrap's Python+mitmproxy stack with a Lua plugin running inside [proxelar](https://github.com/emanuele-em/proxelar) (Rust MITM proxy).

**Status:** investigation complete. Conclusion: **the Lua plugin works correctly for request transformation, but proxelar buffers response bodies whenever a script is loaded — which collapses Claude's token streaming UX into a single end-of-stream dump.** Strategy A (Lua plugin) is not viable for daily use; the work documented here is the spec for Strategy B (own binary on top of `proxyapi` / `hudsucker` / `goproxy`).

---

## Why the pivot was attempted

Constraints surfaced during the session:

1. cc-flytrap.py runs under Python mitmproxy's cask binary, which on this machine could not open outbound TCP from its own process (every upstream CONNECT returned `502 Bad Gateway` / `error establishing server connection: client disconnected`), forcing the user to remove proxy env from `~/.claude.json`.
2. Distribution criterion: "redistribute like rabbits, run on a 100MB RAM chip" — Python is out (mitmproxy alone is 150-200 MB resident, plus interpreter).
3. The user wanted to evaluate alternatives independently before committing to a full rewrite.

After comparing Go (`elazarl/goproxy`, `google/martian`, `lqqyt2423/go-mitmproxy`) and Rust (`hudsucker`, `proxelar`, `third-wheel`, `http-mitm-proxy`) options, **proxelar** stood out: it ships as a binary *and* a library (`proxyapi` crate), already has a Lua hook model with `on_request` / `on_response`, includes a built-in TUI, and is actively maintained. The pilot tested whether cc-flytrap could be reduced to a Lua plugin shipped against `brew install proxelar`.

---

## Gating test 1 — does Anthropic accept HTTP/1.1?

Open question because most Go/Rust MITM libraries have weaker HTTP/2 stories than mitmproxy. If we can force h1.1 on both legs, the h2 maturity gap doesn't matter.

```bash
curl -sS --http1.1 -o /tmp/h1-body \
  -w "HTTP %{http_code}  proto=%{http_version}  total=%{time_total}s  tls=%{time_appconnect}s\n" \
  -m 10 -X POST https://api.anthropic.com/v1/messages \
  -H "content-type: application/json" \
  -H "x-api-key: invalid-test-key" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"claude-haiku-4-5-20251001","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}'
```

Result:

```
HTTP 401  proto=1.1  total=3.771830s  tls=3.454343s
{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"},"request_id":"req_011CamwyDMgmER1aFbwR5Bo6"}
```

Verbose ALPN trace:

```
* ALPN: curl offers http/1.1
* SSL connection using TLSv1.3 / AEAD-CHACHA20-POLY1305-SHA256
* ALPN: server accepted http/1.1
* using HTTP/1.x
< HTTP/1.1 405 Method Not Allowed
< Connection: keep-alive
```

**Verdict:** Anthropic accepts HTTP/1.1 cleanly via ALPN, full JSON body, keep-alive. The h2 question is mooted by `--no-http2` style configuration on the proxy.

---

## Plugin port — `cc-flytrap.lua`

Single self-contained file. Vendors `rxi/json.lua` (MIT, ~250 lines) inline so the plugin ships as one drop-in. Implements only the request-side transformation from `cc-flytrap.py`:

```lua
local PAIN_ENABLED = false   -- false = trim bloat (default cc-flytrap behavior)

function on_request(request)
  if request.method ~= "POST" then return end
  if not request.url:find("api%.anthropic%.com", 1, false) then return end
  if not request.url:find("/v1/messages") then return end
  if not request.body or #request.body == 0 then return end

  local ok, data = pcall(json.decode, request.body)
  if not ok or type(data) ~= "table" then return end
  if type(data.system) ~= "table" then return end

  -- (1) Always: append system_override as additive block
  table.insert(data.system, { type = "text", text = SYSTEM_OVERRIDE })

  -- (2) Only when pain disabled: trim Claude Code's bloat blocks
  if not PAIN_ENABLED then
    if data.system[2] then data.system[2].text = TRIMMED_BLOCK_2 end
    if data.system[3] then data.system[3].text = TRIMMED_BLOCK_3 end
    if data.system[4] then data.system[4].text = TRIMMED_BLOCK_4 end
  end

  local new_body = json.encode(data)
  request.body = new_body
  request.headers["content-length"] = tostring(#new_body)
  return request
end
```

Run:

```bash
proxelar -i terminal -p 7178 -s /Users/adil/cc-flytrap/lua/cc-flytrap.lua
```

**Sanity test** — POST a faux `/v1/messages` body through proxelar with placeholder system blocks and an invalid key:

```bash
HTTPS_PROXY=http://127.0.0.1:7178 \
  curl -sS --http1.1 \
  --cacert ~/.proxelar/proxelar-ca.pem \
  -X POST https://api.anthropic.com/v1/messages \
  -H "x-api-key: invalid-test-key" \
  -H "anthropic-version: 2023-06-01" \
  -H "content-type: application/json" \
  -d '{"system":[{"type":"text","text":"BLOCK1"},{"type":"text","text":"BLOCK2"},...],"messages":[...]}'
```

Plugin log:

```
[cc-flytrap] modified: Override:+1block,Block2,Block3,Block4 (body 1346->1346 bytes)
[01:35:08] #1 POST 401 https://api.anthropic.com/v1/messages (130B)
```

**Result:** request transformation works end-to-end. All four mutations fire (override appended + three blocks trimmed). Anthropic returns its normal 401, proving the modified body parsed cleanly upstream.

---

## Phase 0 — does proxelar break SSE streaming?

The risk: if proxelar buffers the upstream SSE response before invoking `on_response`, the client (Claude) loses token-by-token streaming UX. Tested with a controlled local SSE source and then with real Claude.

### Local SSE source

```python
# /tmp/sse_server.py — emits 5 events at 500ms intervals over HTTP/1.1 chunked
class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Transfer-Encoding", "chunked")
        self.send_header("Connection", "close")
        self.end_headers()
        for i in range(5):
            payload = f"data: {{\"n\":{i},\"ts\":{time.time():.3f}}}\n\n".encode()
            chunk = f"{len(payload):x}\r\n".encode() + payload + b"\r\n"
            self.wfile.write(chunk); self.wfile.flush(); time.sleep(0.5)
        self.wfile.write(b"0\r\n\r\n")
```

Curl with per-chunk timestamps:

```bash
HTTP_PROXY=http://127.0.0.1:7178 curl -sN http://127.0.0.1:9999/events | python3 -u -c "
import sys, time
t0 = time.time(); buf = ''
while True:
    c = sys.stdin.read(1)
    if not c: break
    buf += c
    if c == '\n' and buf.strip():
        print(f'{time.time()-t0:7.3f}s  recv: {buf.strip()}', flush=True); buf = ''
"
```

| Test | Setup | First chunk | Cadence | Verdict |
|---|---|---|---|---|
| 1 | direct curl | 2.038s | +505 / +505 / +503 / +505ms | streams ✓ |
| 2 | proxelar passthrough (HTTP forward, no script) | 0.000s | +488 / +505 / +501 / +506ms | streams ✓ |
| 3 | HTTP forward + script with `on_request` only | 0.000s | +490 / +503 / +504 / +531ms | streams ✓ |
| 4 | HTTP forward + script with `on_response` defined | 0.000s | +523 / +493 / +505 / +503ms | streams ✓ |

Initial conclusion: streaming preserved. **But:** the local tests used HTTP forward mode. A second pass discovered:

```bash
# log_both.lua — print on_request and on_response
# HTTP forward (no MITM) test
HTTP_PROXY=http://127.0.0.1:7178 curl http://127.0.0.1:9998/   # 30B fixed-length
# Result: response correct, BUT proxelar log shows neither hook fired.

# HTTPS MITM test
HTTPS_PROXY=http://127.0.0.1:7178 --cacert ~/.proxelar/proxelar-ca.pem \
  curl https://api.anthropic.com/v1/messages
# Result:
#   [on_request] POST https://api.anthropic.com/v1/messages
#   [on_response] POST https://api.anthropic.com/v1/messages -> 401 (body 130 B)
```

**proxelar's Lua hooks only fire on HTTPS-MITM'd traffic.** Plain HTTP forwarding tunnels bytes without invoking Lua. So the local HTTP tests above proved nothing about the actual production path — they only proved that proxelar streams when it isn't running its hook machinery.

### Real test: Claude through proxelar with HTTPS MITM

The user's `claude` CLI with `--include-partial-messages --output-format stream-json` emits one line per upstream SSE event in real time, line-buffered to stdout. That gives us per-token timing without claude's normal stdout batching.

**Baseline — claude direct, no proxy:**

```
  1.257s  [init]
  1.257s  [status]
  2.914s  [message_start]
  2.914s  [content_block_start]
  2.914s  (+2.914s)  delta   1B
  3.293s  (+0.379s)  delta  56B
  3.772s  (+0.479s)  delta  57B
  4.182s  (+0.410s)  delta  52B
  4.450s  (+0.268s)  delta  93B
  4.824s  (+0.375s)  delta  61B
  5.203s  (+0.378s)  delta  43B
  5.581s  (+0.378s)  delta  53B
  5.962s  (+0.381s)  delta  76B
  6.434s  (+0.472s)  delta  46B
  6.759s  (+0.325s)  delta  96B
  6.905s  (+0.146s)  delta  23B
  6.919s  [content_block_stop]
```

Eleven deltas spread over ~4 seconds, ~300-450 ms apart. Token-by-token streaming as expected.

**Through proxelar with `cc-flytrap.lua` loaded (only `on_request` defined):**

```
  1.491s  [init]
  1.491s  [status]
  6.952s  [message_start]
  6.952s  [content_block_start]
  6.952s  (+6.952s)  delta   1B
  6.952s  (+0.000s)  delta  56B
  6.952s  (+0.000s)  delta  39B
  6.952s  (+0.000s)  delta  85B
  6.953s  (+0.000s)  delta  65B
  6.953s  (+0.000s)  delta  62B
  6.953s  (+0.000s)  delta  56B
  6.953s  (+0.000s)  delta  67B
  6.953s  (+0.000s)  delta  76B
  6.953s  (+0.000s)  delta  73B
  6.953s  (+0.000s)  delta  73B
  6.954s  [content_block_stop]
```

**All eleven deltas land within 1 ms of each other at 6.952s.** The entire 4-second stream collapses to a single instant at end-of-stream. The user sees nothing for ~7s, then everything at once.

**Through proxelar with NO script loaded (HTTPS MITM passthrough):**

```
  3.419s  (+3.419s)  delta
  3.420s  (+0.000s)  delta
  3.847s  (+0.427s)  delta
  4.285s  (+0.438s)  delta
  4.695s  (+0.410s)  delta
  4.955s  (+0.260s)  delta
  5.414s  (+0.459s)  delta
  5.690s  (+0.276s)  delta
  6.071s  (+0.381s)  delta
  6.457s  (+0.386s)  delta
  6.778s  (+0.322s)  delta
```

Streaming preserved — same cadence as direct.

### Buffering matrix

| proxelar config | HTTPS MITM | hooks fire | streams to client |
|---|---|---|---|
| No script | yes | n/a | **yes** |
| Script with `on_request` only | yes | yes | **no** (buffered ~4s) |
| Script with `on_response` only | yes | yes | **no** (buffered ~4s) |
| Script with both | yes | yes | **no** (buffered ~4s) |
| HTTP forward (any script) | no MITM | **no** | yes (raw tunnel) |

**Trigger is script load, not the specific hook set.** Once a script is registered, proxelar collects the full response body before any client-side delivery. Likely a deliberate simplification at the mlua call boundary.

---

## Other findings worth recording

### Response bodies are gzipped on the wire

`response.body` in Lua hooks is the raw wire bytes. Anthropic responds with `Content-Encoding: gzip`. A 591B "ok" SSE response in our logs was 2-5KB uncompressed.

```
[on_response] POST https://api.anthropic.com/v1/messages -> 401 (body 130 B)
  body preview: �      �W�n�:�=ێԋ��� ...   ← gzip magic 0x1f 0x8b
```

**Workaround for ledger plan:** set `request.headers["accept-encoding"] = "identity"` in `on_request` for `/v1/messages` so the response comes back uncompressed. Adds bandwidth but avoids vendoring a Lua gzip lib (heavy — ~500 LOC of pure-Lua deflate).

### `on_response` request context is minimal

```rust
// from proxyapi/src/scripting.rs
let req_table = lua.create_table().and_then(|t| {
    t.set("method", req_method)?;
    t.set("url", req_url)?;
    Ok(t)
})
```

Only `{method, url}` — no headers, no body, no flow ID. Threading session_id from `on_request` to `on_response` requires a Lua-level stash table keyed by URL with a queue per URL to handle concurrent same-URL requests.

### Claude works end-to-end

```bash
HTTPS_PROXY=http://127.0.0.1:7178 \
NODE_EXTRA_CA_CERTS=$HOME/.proxelar/proxelar-ca.pem \
  claude -p "Just say: ok"
# → "ok"
```

OAuth flow works. CA at `~/.proxelar/proxelar-ca.pem` (auto-generated on first run). 12 different Anthropic endpoints get hit on a normal `claude -p` (mcp_servers, claude_code_penguin_mode, claude_cli/bootstrap, oauth/account/settings, mcp-registry, /v1/messages, event_logging, etc.) — all proxied successfully.

### proxelar facts

- **Install:** `brew install proxelar` — 7.9 MB binary
- **License:** MIT
- **TLS:** rustls + ring (server side); openssl-sys with `vendored` feature for cert generation
- **HTTP:** hyper 1.x with `["http1", "server", "client"]` features only — **no h2 feature enabled**, h1.1-only by design
- **Lua engine:** mlua 0.11 with Lua 5.4, vendored, `send` feature. No sandboxing — full stdlib (`io`, `os`, `package.path`).
- **Shape:** workspace with three crates: `proxelar-cli` (binary), `proxyapi` (library — published independently on crates.io), `proxyapi_models`. Strategy B can depend on `proxyapi` directly.
- **Stars / users:** 963 stars, 176 brew installs in 90 days. Single-maintainer (bus factor real).

---

## Implications for Strategy B

The Lua port did its job. We've validated:

- HTTP/1.1 to Anthropic works with full functionality
- Request transformation is ~30 lines of logic (JSON parse + mutate `system` array + re-encode)
- Ledger is well-defined: SSE walk + token aggregation across `message_start` / `message_delta` events with `usage` field (logic is in `cc-flytrap.py`)
- claude+OAuth integrates cleanly with a CA-installing MITM proxy
- The "rabbits" distribution criterion is achievable in Rust at ~10 MB binary, well under 100 MB resident

What we couldn't fix in Strategy A: **proxelar's response-body buffering on script-load.** This is architectural — it's not a config knob.

Strategy B candidates, evaluated against streaming-while-mutating requirement:

1. **Build on `proxyapi` crate, bypass its `ScriptEngine`.** Tightest integration with proxelar ecosystem. Need to register custom handlers at hyper level rather than going through the Lua hook machinery. Inherits proxelar's TLS+CA infrastructure for free.
2. **Build on `omjadas/hudsucker`** — modern hyper 1.x + tokio + rustls foundation. No inherited buffering decisions. Open h2 bug (#145) is moot in h1-only mode. Apache/MIT.
3. **Build on `elazarl/goproxy`** (Go side) — biggest community (6.7k stars, 2.1k dependent projects), faster ship loop, easier cross-compile, smaller distribution friction. Open h2 PR (#772) is moot in h1-only mode.

Decision pending. If the redistribution-friction criterion dominates, Go wins. If Rust day-to-day ergonomics + ratatui matter more, hudsucker wins.

---

## Files in this directory

- `cc-flytrap.lua` — the plugin (~200 lines, vendors rxi/json.lua inline). Self-contained. Works correctly for request transformation. Buffers responses (proxelar's choice, not ours).
- `README.md` — this document.

The plugin is left in place because:

1. It's a working spec for the Strategy B port.
2. It's useful for one-off "I want to apply system_override to a single claude session and don't care about streaming UX for those few minutes."
3. The vendored JSON code is a reference if any other Lua plugin work happens.

To run a one-off:

```bash
proxelar -i terminal -p 7178 -s /Users/adil/cc-flytrap/lua/cc-flytrap.lua &
HTTPS_PROXY=http://127.0.0.1:7178 \
NODE_EXTRA_CA_CERTS=$HOME/.proxelar/proxelar-ca.pem \
  claude "..."
```

---

## Sources

- [emanuele-em/proxelar](https://github.com/emanuele-em/proxelar)
- [proxelar Lua scripting source — proxyapi/src/scripting.rs](https://github.com/emanuele-em/proxelar/blob/main/proxyapi/src/scripting.rs)
- [proxelar Homebrew formula](https://formulae.brew.sh/api/formula/proxelar.json)
- [rxi/json.lua](https://github.com/rxi/json.lua) — vendored
- [omjadas/hudsucker](https://github.com/omjadas/hudsucker)
- [hudsucker issue #145 — HTTP/2 + TLS client↔proxy](https://github.com/omjadas/hudsucker/issues/145)
- [elazarl/goproxy](https://github.com/elazarl/goproxy)
- [goproxy PR #772 — terminate HTTP/2 sessions instead of forwarding raw frames](https://github.com/elazarl/goproxy/issues/772)
- [google/martian](https://github.com/google/martian) — archived
- [lqqyt2423/go-mitmproxy](https://github.com/lqqyt2423/go-mitmproxy)
