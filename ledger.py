#!/usr/bin/env python3
"""
ccft Ledger - API request audit trail and bookkeeping module
Stores individual request records for time series analysis and auditing
v2.1 - Full vectors, JSONL, microsecond timestamps
"""

import json
import os
import time
import socket
import uuid
from pathlib import Path
from urllib.request import urlopen
from urllib.error import URLError

ledger_file = Path(os.environ.get('CCFT_LEDGER', str(Path.home() / '.local' / 'share' / 'ccft' / 'ledger.jsonl')))
state_file = ledger_file.parent / 'state.jsonl'
ledger_file.parent.mkdir(parents=True, exist_ok=True)

# Agent instance info - computed once
AGENT_INSTANCE = socket.gethostname() + "-" + str(uuid.uuid4())[:8]
HUMAN = os.environ.get('USER', 'unknown')

# Public IP cache
_public_ip = None
_public_ip_fetched = None
_public_ip_cache_seconds = 3600

def get_public_ip():
    global _public_ip, _public_ip_fetched
    now = time.time()
    
    if _public_ip and _public_ip_fetched and (now - _public_ip_fetched) < _public_ip_cache_seconds:
        return _public_ip
    
    try:
        _public_ip = urlopen("https://api.ipify.org", timeout=5).read().decode()
        _public_ip_fetched = now
    except URLError:
        _public_ip = None
    
    return _public_ip

def load_records():
    records = []
    if ledger_file.exists():
        try:
            with open(ledger_file) as f:
                for line in f:
                    line = line.strip()
                    if line:
                        records.append(json.loads(line))
        except:
            pass
    return records

def get_stats():
    records = load_records()
    if not records:
        return {
            "total_requests": 0,
            "total_input_tokens": 0,
            "total_output_tokens": 0,
            "total_latency_ms": 0
        }
    
    return {
        "total_requests": len(records),
        "total_input_tokens": sum(r.get("in", 0) for r in records),
        "total_output_tokens": sum(r.get("out", 0) for r in records),
        "total_latency_ms": sum(r.get("lat", 0) for r in records)
    }

def add(
    model=None,
    input_tokens=0,
    output_tokens=0,
    latency_ms=0,
    client_ip=None,
    server_ip=None,
    endpoint=None,
    region=None,
    session_id=None,
    timestamp_start=None,
    timestamp_end=None,
    ccft_us=0,
):
    record = {
        # Time (epoch seconds with microsecond precision)
        "ts": timestamp_start if timestamp_start else time.time(),
        "te": timestamp_end if timestamp_end else time.time(),
        "dt": time.strftime("%Y-%m-%d %H:%M:%S"),

        # Human & Agent
        "human": HUMAN,
        "agent": AGENT_INSTANCE,
        "sid": session_id,

        # Network
        "cip": client_ip,
        "pip": get_public_ip(),
        "sip": server_ip,

        # API context
        "ep": endpoint,
        "reg": region,
        "model": model or "unknown",

        # Tokens & Performance
        "in": input_tokens,
        "out": output_tokens,
        "tot": input_tokens + output_tokens,
        "lat": latency_ms,

        # ccft internal processing time (microseconds). 0 on legacy records.
        "c_us": ccft_us,
    }

    with open(ledger_file, 'a') as f:
        f.write(json.dumps(record) + '\n')

    return get_stats()

def record_state(event, **extra):
    """Append a state-transition event to state.jsonl.

    Used by brainrot to know when the ledger was active vs not, so quiet
    periods in the request stream can be distinguished from off-periods.

    Events:
      'ledger_on'   ledger recording enabled at script load
      'ledger_off'  ledger recording disabled at script load
    """
    record = {
        "ts": time.time(),
        "dt": time.strftime("%Y-%m-%d %H:%M:%S"),
        "event": event,
        "human": HUMAN,
        "agent": AGENT_INSTANCE,
    }
    record.update(extra)
    state_file.parent.mkdir(parents=True, exist_ok=True)
    with open(state_file, 'a') as f:
        f.write(json.dumps(record) + '\n')


def load_state_events():
    """Read all state events in chronological order. Empty list if no file."""
    if not state_file.exists():
        return []
    events = []
    with open(state_file) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    events.sort(key=lambda e: e.get('ts', 0))
    return events




def reset():
    if ledger_file.exists():
        ledger_file.unlink()

def records():
    return load_records()

def stats():
    return get_stats()