"""Cross-language hash parity is asserted in apps/api-ai integration tests."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from strategy_spec import (
    SchemaValidationError,
    canonicalize,
    hash_spec,
    semantic_check,
    validate,
    with_hash,
)
from strategy_spec.models import StrategySpec


FIXTURE = Path(__file__).resolve().parents[2] / "fixtures" / "wyckoff_spring_btc_15m.json"


@pytest.fixture
def spec() -> dict:
    return json.loads(FIXTURE.read_text("utf-8"))


def test_golden_validates(spec: dict) -> None:
    validate(spec)
    StrategySpec.model_validate(spec)


def test_canonical_is_key_order_invariant() -> None:
    a = {"b": 1, "a": 2, "c": {"y": 1, "x": 2}}
    b = {"c": {"x": 2, "y": 1}, "a": 2, "b": 1}
    assert canonicalize(a) == canonicalize(b)


def test_hash_is_stable(spec: dict) -> None:
    h1 = hash_spec(spec)
    h2 = hash_spec(spec)
    assert h1 == h2
    assert len(h1) == 64
    assert all(c in "0123456789abcdef" for c in h1)


def test_hash_matches_pinned_cross_language_value(spec: dict) -> None:
    pinned_path = FIXTURE.with_name(FIXTURE.stem + ".hash.txt")
    pinned = pinned_path.read_text("utf-8").strip()
    assert hash_spec(spec) == pinned


def test_with_hash_round_trip(spec: dict) -> None:
    sealed = with_hash(spec)
    assert sealed["spec_hash"] == hash_spec(spec)


def test_semantic_check_passes_on_golden(spec: dict) -> None:
    assert semantic_check(spec) == []


def test_semantic_check_rejects_unknown_id(spec: dict) -> None:
    spec["entries"][0]["when"] = "wy_spring && does_not_exist"
    problems = semantic_check(spec)
    assert any("does_not_exist" in p.message for p in problems)


def test_validation_rejects_float_per_trade_pct(spec: dict) -> None:
    spec["risk"]["per_trade_pct"] = 0.5  # number instead of decimal-string
    with pytest.raises(SchemaValidationError):
        validate(spec)
