"""Canonical JSON + spec hashing.

Must produce byte-identical output to the TypeScript and Rust implementations:
  - recursive lexicographic key sort
  - separators ',' and ':'  (no whitespace)
  - non-ASCII escaped as \\uXXXX (Python's json default)
  - array order preserved
  - integer-valued floats are normalized to ints (so 3.0 == 3) — matches JS Number behavior
  - sha256, lowercase hex
"""

from __future__ import annotations

import hashlib
import json
import math
from typing import Any


def _normalize(value: Any) -> Any:
    """Recursively coerce integer-valued floats to ints. JS lacks a float/int distinction
    and JSON.stringify(3.0) → "3", so Python/Rust must drop the trailing .0 to match."""
    if isinstance(value, bool):
        return value
    if isinstance(value, float):
        if math.isfinite(value) and value.is_integer():
            return int(value)
        return value
    if isinstance(value, dict):
        return {k: _normalize(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_normalize(v) for v in value]
    return value


def canonicalize(value: Any) -> str:
    """Return canonical JSON for `value`."""
    return json.dumps(_normalize(value), sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def hash_spec(spec: dict[str, Any]) -> str:
    """sha256 of canonical JSON of `spec` with `spec_hash` removed."""
    body = {k: v for k, v in spec.items() if k != "spec_hash"}
    return hashlib.sha256(canonicalize(body).encode("utf-8")).hexdigest()


def with_hash(spec: dict[str, Any]) -> dict[str, Any]:
    """Return a copy of `spec` with `spec_hash` set to its canonical hash."""
    return {**spec, "spec_hash": hash_spec(spec)}
