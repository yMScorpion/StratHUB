"""Canonical JSON + spec hashing.

Must produce byte-identical output to the TypeScript and Rust implementations:
  - recursive lexicographic key sort
  - separators ',' and ':'  (no whitespace)
  - non-ASCII preserved as UTF-8 (ensure_ascii=False matches JS/Rust behaviour)
  - floats formatted to match JS JSON.stringify: decimal for abs in [1e-6, 1e21),
    scientific with normalised exponent (no leading zeros) outside that range
  - array order preserved
  - integer-valued floats are normalized to ints (so 3.0 == 3) — matches JS Number behavior
  - sha256, lowercase hex
"""

from __future__ import annotations

import decimal
import hashlib
import json
import math
import re
from typing import Any

# Strips leading zeros from a scientific-notation exponent: e-07 → e-7, e+03 → e+3.
# Used only for numbers that JS also represents in scientific notation.
_EXPONENT_LEAD_ZERO_RE = re.compile(r'e([+-])0+([1-9]\d*)', re.IGNORECASE)

# Matches either a JSON string literal (group 1 absent → pass through unchanged) or a
# scientific-notation number token (group 1 present → reformat to match JS output).
# The string branch must come first so that e-notation inside quoted values is never touched.
_CANONICAL_RE = re.compile(
    r'"(?:[^"\\]|\\.)*"'            # JSON string literal — skip
    r'|(-?\d+\.?\d*[eE][+-]?\d+)',  # scientific-notation number
)


def _normalize(value: Any) -> Any:
    """Recursively coerce integer-valued floats to ints. JS lacks a float/int distinction
    and JSON.stringify(3.0) → "3", so Python/Rust must drop the trailing .0 to match."""
    if isinstance(value, bool):
        return value
    if isinstance(value, float):
        # JS JSON.stringify uses scientific notation for abs >= 1e21 (n > 21 in ECMAScript).
        # Converting those to Python int would produce decimal form (wrong). Only convert
        # integer-valued floats that JS would also format as a decimal integer.
        if math.isfinite(value) and value.is_integer() and abs(value) < 1e21:
            return int(value)
        return value
    if isinstance(value, dict):
        return {k: _normalize(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_normalize(v) for v in value]
    return value


def _js_number(raw: str) -> str:
    """Reformat a scientific-notation JSON number to match JS JSON.stringify output.

    JS uses decimal for numbers where ECMAScript's n value satisfies -6 < n <= 21,
    and scientific (with no exponent sign for negatives, + for positives) otherwise.
    Python's json.dumps switches to scientific much earlier (~1e-5), so we must convert
    the affected range to decimal here.
    """
    f = float(raw)
    if f == 0.0:
        return "0"
    sign = "-" if f < 0.0 else ""
    d = decimal.Decimal(repr(abs(f)))
    tup = d.as_tuple()
    # ECMAScript n: position of the most-significant digit relative to the decimal point
    n = len(tup.digits) + tup.exponent
    if -6 < n <= 21:
        # JS uses decimal notation for this range
        return sign + format(d, 'f')
    # JS uses scientific; normalise exponent (strip leading zeros added by Python/C)
    return _EXPONENT_LEAD_ZERO_RE.sub(r'e\1\2', repr(f))


def canonicalize(value: Any) -> str:
    """Return canonical JSON for `value`."""
    raw = json.dumps(_normalize(value), sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return _CANONICAL_RE.sub(
        lambda m: m.group(0) if m.group(1) is None else _js_number(m.group(1)),
        raw,
    )


def hash_spec(spec: dict[str, Any]) -> str:
    """sha256 of canonical JSON of `spec` with `spec_hash` removed."""
    body = {k: v for k, v in spec.items() if k != "spec_hash"}
    return hashlib.sha256(canonicalize(body).encode("utf-8")).hexdigest()


def with_hash(spec: dict[str, Any]) -> dict[str, Any]:
    """Return a copy of `spec` with `spec_hash` set to its canonical hash."""
    return {**spec, "spec_hash": hash_spec(spec)}
