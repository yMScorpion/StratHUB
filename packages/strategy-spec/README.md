# strategy-spec

The canonical Strategy Spec — a constrained JSON DSL produced by the AI pipeline and consumed (immutably) by the backtest, paper, and live execution paths.

**JSON Schema in `schema/strategy-spec.schema.json` is the single source of truth.** TS, Pydantic, and serde models are kept as thin language bindings. Fixtures in `fixtures/` are validated against the schema in CI.

## Why a DSL, not Python

LLM-generated Python is never executed. The compiler emits this JSON; the Rust executor interprets it. Same spec → same semantics in backtest, paper, and live.

## Canonical hash

`spec_hash = sha256(canonical_json(spec_without_hash))` where canonical_json:
- recursively sorts object keys lexicographically,
- uses `,` and `:` separators (no whitespace),
- preserves array order (semantically meaningful),
- encodes non-ASCII as escaped `\uXXXX` (default JSON behavior),
- uses lowercase hex for the digest.

The hash is stable across TS, Python, and Rust implementations and is the reproducibility key for backtests, validation runs, and live deployments.

## Layout

```
schema/strategy-spec.schema.json   single source of truth
fixtures/                          golden examples (validated in CI)
ts/                                TypeScript binding (zod-free, schema-driven)
py/                                Pydantic v2 + ajv-style validation
rs/                                serde + jsonschema validation
```
