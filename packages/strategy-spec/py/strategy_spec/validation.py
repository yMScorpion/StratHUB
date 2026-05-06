"""Schema and semantic validation for the Strategy Spec."""

from __future__ import annotations

import importlib.resources
import json
import re
from dataclasses import dataclass
from functools import cache
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator, FormatChecker
from jsonschema.exceptions import ValidationError


class SchemaValidationError(Exception):
    def __init__(self, errors: list[ValidationError]) -> None:
        self.errors = errors
        super().__init__(
            "; ".join(f"{list(e.absolute_path)}: {e.message}" for e in errors[:5])
        )


@dataclass(frozen=True)
class SemanticProblem:
    code: str
    message: str
    path: str | None = None


def get_schema() -> dict[str, Any]:
    """Return the parsed JSON Schema for the Strategy Spec (public API)."""
    return _load_schema()


@cache
def _load_schema() -> dict[str, Any]:
    # When installed as a wheel, the schema is force-included beside this module.
    pkg_root = Path(__file__).resolve().parent
    bundled = pkg_root / "_schema.json"
    if bundled.is_file():
        return json.loads(bundled.read_text("utf-8"))

    # Fallback for local dev / editable installs: walk up to find packages/strategy-spec/schema/.
    here = Path(__file__).resolve()
    for parent in here.parents:
        candidate = parent / "schema" / "strategy-spec.schema.json"
        if candidate.is_file():
            return json.loads(candidate.read_text("utf-8"))

    # Last resort — importlib resources within the installed wheel.
    schema_text = importlib.resources.read_text("strategy_spec", "_schema.json")
    return json.loads(schema_text)


@cache
def _validator() -> Draft202012Validator:
    # FormatChecker enables validation of `format: uuid` (and others) declared in the schema.
    return Draft202012Validator(_load_schema(), format_checker=FormatChecker())


def validate(spec: dict[str, Any]) -> dict[str, Any]:
    """JSON-Schema validate. Raises SchemaValidationError on failure. Returns the input on success."""
    errors = sorted(_validator().iter_errors(spec), key=lambda e: list(e.absolute_path))
    if errors:
        raise SchemaValidationError(errors)
    return spec


_ID_RE = re.compile(r"[a-z][a-z0-9_]{0,31}")
# Tokeniser: longest-match ordered so && and || are consumed before their individual chars.
_WHEN_TOKEN_RE = re.compile(r"&&|\|\||!|\(|\)|[a-z][a-z0-9_]{0,31}")


class _ParseError(Exception):
    pass


def _when_syntax_problems(when: str, index: int) -> list[SemanticProblem]:
    """Parse `when` as a boolean expression; return problems on invalid syntax.

    Grammar:
        expr    ::= or
        or      ::= and ('||' and)*
        and     ::= not ('&&' not)*
        not     ::= '!' not | primary
        primary ::= ID | '(' expr ')'
    """
    path = f"entries[{index}].when"
    stripped = when.strip()

    # Tokenise — any character not consumed by the token regex is illegal.
    tokens: list[str] = []
    pos = 0
    while pos < len(stripped):
        if stripped[pos].isspace():
            pos += 1
            continue
        m = _WHEN_TOKEN_RE.match(stripped, pos)
        if m:
            tokens.append(m.group())
            pos = m.end()
        else:
            bad = stripped[pos:pos + 4]
            return [SemanticProblem("invalid_when_syntax", f"entries[{index}].when: unexpected token {bad!r}", path)]

    if not tokens:
        return [SemanticProblem("invalid_when_syntax", f"entries[{index}].when cannot be empty", path)]

    cursor = [0]

    def peek() -> str | None:
        return tokens[cursor[0]] if cursor[0] < len(tokens) else None

    def consume() -> str:
        tok = tokens[cursor[0]]
        cursor[0] += 1
        return tok

    def parse_or() -> None:
        parse_and()
        while peek() == "||":
            consume()
            parse_and()

    def parse_and() -> None:
        parse_not()
        while peek() == "&&":
            consume()
            parse_not()

    def parse_not() -> None:
        if peek() == "!":
            consume()
            parse_not()
        else:
            parse_primary()

    def parse_primary() -> None:
        t = peek()
        if t is None:
            raise _ParseError("unexpected end of expression")
        if t == "(":
            consume()
            parse_or()
            if peek() != ")":
                raise _ParseError("expected ')'")
            consume()
        elif _ID_RE.fullmatch(t):
            consume()
        else:
            raise _ParseError(f"unexpected token {t!r}")

    try:
        parse_or()
        if cursor[0] < len(tokens):
            raise _ParseError(f"unexpected token {tokens[cursor[0]]!r}")
    except _ParseError as exc:
        return [SemanticProblem("invalid_when_syntax", f"entries[{index}].when: {exc}", path)]

    return []


def semantic_check(spec: dict[str, Any]) -> list[SemanticProblem]:
    """Beyond JSON-Schema invariants. Returns an empty list on success."""
    problems: list[SemanticProblem] = []

    indicators = spec.get("indicators", [])
    patterns = spec.get("patterns", [])
    indicator_ids = [i["id"] for i in indicators]
    pattern_ids = [p["id"] for p in patterns]

    if len(indicator_ids) != len(set(indicator_ids)):
        problems.append(SemanticProblem("dup_indicator_id", "indicator ids must be unique", "indicators"))
    if len(pattern_ids) != len(set(pattern_ids)):
        problems.append(SemanticProblem("dup_pattern_id", "pattern ids must be unique", "patterns"))

    known = set(indicator_ids) | set(pattern_ids)
    reserved = {"true", "false"}
    for i, entry in enumerate(spec.get("entries", [])):
        when = entry["when"]
        problems.extend(_when_syntax_problems(when, i))
        for ref in _ID_RE.findall(when):
            if ref in reserved:
                continue
            if ref not in known:
                problems.append(
                    SemanticProblem(
                        "unknown_id",
                        f'entries[{i}].when references unknown id "{ref}"',
                        f"entries[{i}].when",
                    )
                )

    risk = spec.get("risk", {})
    if "min_rr" in risk:
        try:
            if float(risk["min_rr"]) < 3.0:
                problems.append(SemanticProblem(
                    "min_rr_below_three",
                    "risk.min_rr must be >= 3 (Apostila methodology requires >= 3)",
                    "risk.min_rr",
                ))
        except ValueError:
            pass

    return problems
