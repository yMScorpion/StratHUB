"""Pydantic v2 models that mirror the JSON Schema. The schema is the source of truth — these
models exist for ergonomic Python access and IDE completion only."""

from __future__ import annotations

from decimal import Decimal
from typing import Annotated, Literal
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field, StringConstraints

AssetClass = Literal["spot", "perp"]
Exchange = Literal["binance", "bybit"]
Timeframe = Literal["1m", "3m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "12h", "1d"]

DecimalStr = Annotated[str, StringConstraints(pattern=r"^-?[0-9]+(\.[0-9]+)?$")]
PositiveDecimalStr = Annotated[str, StringConstraints(pattern=r"^[0-9]+(\.[0-9]+)?$")]
Id = Annotated[str, StringConstraints(pattern=r"^[a-z][a-z0-9_]{0,31}$")]
RuleRef = Annotated[
    str,
    StringConstraints(
        pattern=r"^(risk(\.[a-z_]+)?|(entries|exits|filters|patterns|indicators)\[[0-9]+\](\.[a-z_]+)?)$"
    ),
]
Symbol = Annotated[str, StringConstraints(pattern=r"^[A-Z0-9]{2,20}$")]


class _Frozen(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True, str_strip_whitespace=False)


class Indicator(_Frozen):
    id: Id
    kind: Literal["sma", "ema", "rsi", "atr", "macd", "bb", "vwap", "volume_ma", "obv", "adx"]
    params: dict[str, float]
    source: Literal["close", "open", "high", "low", "hlc3", "ohlc4", "volume"] | None = None


class Pattern(_Frozen):
    id: Id
    kind: Literal[
        "wyckoff_spring",
        "wyckoff_upthrust",
        "wyckoff_phase_a",
        "wyckoff_phase_b",
        "wyckoff_phase_c",
        "wyckoff_phase_d",
        "wyckoff_phase_e",
        "vsa_no_supply",
        "vsa_no_demand",
        "vsa_stopping_volume",
        "vsa_climactic_volume",
        "vsa_effort_no_result",
        "vsa_test_bar",
        "order_block",
        "fair_value_gap",
        "liquidity_sweep",
        "break_of_structure",
        "change_of_character",
    ]
    params: dict[str, float | str | bool]


class Filter(_Frozen):
    kind: Literal["session", "regime", "volatility", "min_price", "max_spread_bps", "weekday"]
    params: dict[str, float | str | bool]


class Size(_Frozen):
    kind: Literal["risk_pct", "fixed_pct", "fixed_notional", "kelly_fraction", "vol_target"]
    value: DecimalStr
    kelly_cap: PositiveDecimalStr | None = None

    def value_decimal(self) -> Decimal:
        return Decimal(self.value)


class Entry(_Frozen):
    side: Literal["long", "short"]
    when: Annotated[str, StringConstraints(min_length=1, max_length=512)]
    size: Size
    max_concurrent_per_symbol: Annotated[int, Field(ge=1, le=10)] | None = None


class Exit(_Frozen):
    kind: Literal[
        "stop_atr",
        "stop_pct",
        "stop_swing",
        "tp_rr",
        "tp_pct",
        "tp_atr",
        "trailing",
        "time_stop",
        "indicator_cross",
    ]
    params: dict[str, float | str | bool]


class Risk(_Frozen):
    model: Literal["fixed_pct", "fixed_notional", "kelly_fraction", "vol_target"]
    per_trade_pct: PositiveDecimalStr
    max_concurrent: Annotated[int, Field(ge=1, le=32)]
    max_daily_loss_pct: PositiveDecimalStr | None = None
    min_rr: PositiveDecimalStr | None = None


class Citation(_Frozen):
    rule_ref: RuleRef
    pdf_id: UUID
    pages: Annotated[list[Annotated[int, Field(ge=1)]], Field(min_length=1, max_length=16)]
    quote: Annotated[str, StringConstraints(max_length=1024)] | None = None


class StrategySpec(_Frozen):
    spec_version: Annotated[str, StringConstraints(pattern=r"^1\.[0-9]+\.[0-9]+$")]
    spec_hash: Annotated[str, StringConstraints(pattern=r"^[0-9a-f]{64}$")] | None = None
    asset_class: AssetClass
    exchange: Exchange
    symbols: Annotated[list[Symbol], Field(min_length=1, max_length=8)]
    timeframe: Timeframe
    indicators: Annotated[list[Indicator], Field(max_length=32)]
    patterns: Annotated[list[Pattern], Field(max_length=32)]
    filters: Annotated[list[Filter], Field(max_length=16)]
    entries: Annotated[list[Entry], Field(min_length=1, max_length=8)]
    exits: Annotated[list[Exit], Field(min_length=1, max_length=16)]
    risk: Risk
    citations: Annotated[list[Citation], Field(min_length=1)]
    metadata: dict[str, str] | None = None
