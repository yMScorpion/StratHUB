"""Canonical JSON + spec hashing.

Must produce byte-identical output to the TypeScript and Rust implementations:
  - recursive lexicographic key sort
  - separators ',' and ':'  (no whitespace)
  - non-ASCII preserved as UTF-8 (ensure_ascii=False matches JS/Rust behaviour)
  - scientific notation exponent leading zeros stripped (1e-07 → 1e-7, matches JS/Rust)
  - array order preserved
  - integer-valued floats are normalized to ints (so 3.0 == 3) — matches JS Number behavior
  - sha256, lowercase hex

Limitation: floats in [5e-7, 1e-5) may still diverge from JS because Python switches to
scientific notation at 1e-5 while JS stays decimal until 1e-7. Indicator params should
stay in the normal decimal range to avoid this edge case.
"""

from __future__ import annotations

import hashlib
import json
import math
import re
from typing import Any

# Matches a scientific-notation exponent with leading zeros: e.g. e-07 → e-7, e+03 → e+3.
# Python json.dumps pads single-digit exponents to two digits; JS/Rust do not.
_EXPONENT_LEAD_ZERO_RE = re.compile(r'e([+-])0+([1-9]\d*)', re.IGNORECASE)


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
    raw = json.dumps(_normalize(value), sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return _EXPONENT_LEAD_ZERO_RE.sub(r'e\1\2', raw)


def hash_spec(spec: dict[str, Any]) -> str:
    """sha256 of canonical JSON of `spec` with `spec_hash` removed."""
    body = {k: v for k, v in spec.items() if k != "spec_hash"}
    return hashlib.sha256(canonicalize(body).encode("utf-8")).hexdigest()


def with_hash(spec: dict[str, Any]) -> dict[str, Any]:
    """Return a copy of `spec` with `spec_hash` set to its canonical hash."""
    return {**spec, "spec_hash": hash_spec(spec)}
