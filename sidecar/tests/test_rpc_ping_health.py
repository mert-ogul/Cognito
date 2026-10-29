"""JSON-RPC ping against sidecar stdin/stdout."""

from __future__ import annotations

import json
import sys
from pathlib import Path
from subprocess import PIPE, Popen


def _py() -> Path:
    root = Path(__file__).resolve().parents[1]
    venv = root / ".venv" / "bin" / "python"
    if venv.exists():
        return venv
    return Path(sys.executable)


def test_ping_line_rpc():
    main = Path(__file__).resolve().parents[1] / "main.py"
    proc = Popen(
        [_py(), str(main)],
        stdin=PIPE,
        stdout=PIPE,
        stderr=PIPE,
        text=True,
        cwd=str(main.parent),
    )
    assert proc.stdin is not None and proc.stdout is not None
    line = json.dumps(
        {"jsonrpc": "2.0", "id": 1, "method": "ping", "params": {}},
    )
    proc.stdin.write(line + "\n")
    proc.stdin.flush()
    out = proc.stdout.readline()
    proc.terminate()
    proc.wait(timeout=5)
    data = json.loads(out)
    assert data.get("result", {}).get("pong") is True
