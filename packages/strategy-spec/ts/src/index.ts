import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import Ajv2020, { type ErrorObject } from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

export type AssetClass = "spot" | "perp";
export type Exchange = "binance" | "bybit";

export interface StrategySpec {
  spec_version: string;
  spec_hash?: string;
  asset_class: AssetClass;
  exchange: Exchange;
  symbols: string[];
  timeframe:
    | "1m" | "3m" | "5m" | "15m" | "30m"
    | "1h" | "2h" | "4h" | "6h" | "8h" | "12h" | "1d";
  indicators: Indicator[];
  patterns: Pattern[];
  filters: Filter[];
  entries: Entry[];
  exits: Exit[];
  risk: Risk;
  citations: Citation[];
  metadata?: Record<string, string>;
}

export interface Indicator { id: string; kind: string; params: Record<string, number>; source?: string; }
export interface Pattern   { id: string; kind: string; params: Record<string, number | string | boolean>; }
export interface Filter    { kind: string; params: Record<string, number | string | boolean>; }
export interface Size      { kind: "risk_pct" | "fixed_pct" | "fixed_notional" | "kelly_fraction" | "vol_target"; value: string; kelly_cap?: string; }
export interface Entry     { side: "long" | "short"; when: string; size: Size; max_concurrent_per_symbol?: number; }
export interface Exit      { kind: string; params: Record<string, number | string | boolean>; }
export interface Risk {
  model: "fixed_pct" | "fixed_notional" | "kelly_fraction" | "vol_target";
  per_trade_pct: string;
  max_concurrent: number;
  max_daily_loss_pct?: string;
  min_rr?: string;
}
export interface Citation  { rule_ref: string; pdf_id: string; pages: number[]; quote?: string; }

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const SCHEMA_PATH = path.resolve(__dirname, "../../schema/strategy-spec.schema.json");
const schema = JSON.parse(readFileSync(SCHEMA_PATH, "utf8")) as object;

const ajv = new Ajv2020({
  allErrors: true,
  strict: true,
  allowUnionTypes: true,
  removeAdditional: false,
  useDefaults: false,
});
addFormats(ajv);
const compiledValidator = ajv.compile<StrategySpec>(schema);

export type ValidationResult =
  | { ok: true; spec: StrategySpec }
  | { ok: false; errors: ErrorObject[] };

export function validate(input: unknown): ValidationResult {
  if (compiledValidator(input)) {
    return { ok: true, spec: input as StrategySpec };
  }
  return { ok: false, errors: compiledValidator.errors ?? [] };
}

/**
 * Canonical JSON: recursively sort object keys, no whitespace, preserve array order.
 * Must match the Python and Rust implementations byte-for-byte.
 * Non-ASCII is preserved as UTF-8 (JSON.stringify passthrough, matching Python ensure_ascii=False).
 */
export function canonicalize(value: unknown): string {
  const sorter = (v: unknown): unknown => {
    if (v === null || typeof v !== "object") return v;
    if (Array.isArray(v)) return v.map(sorter);
    const out: Record<string, unknown> = {};
    for (const k of Object.keys(v as Record<string, unknown>).sort()) {
      out[k] = sorter((v as Record<string, unknown>)[k]);
    }
    return out;
  };
  return JSON.stringify(sorter(value));
}

export function hash(spec: StrategySpec): string {
  const { spec_hash: _omit, ...rest } = spec;
  return createHash("sha256").update(canonicalize(rest), "utf8").digest("hex");
}

export function withHash(spec: StrategySpec): StrategySpec {
  return { ...spec, spec_hash: hash(spec) };
}

// Tokeniser: longest-match ordered so && / || are consumed before their individual chars.
const WHEN_TOKEN_RE = /^(&&|\|\||!|\(|\)|[a-z][a-z0-9_]{0,31})/;

function tokenizeWhen(stripped: string): string[] | { bad: string } {
  const tokens: string[] = [];
  let pos = 0;
  while (pos < stripped.length) {
    if (/\s/.test(stripped[pos])) { pos++; continue; }
    const m = stripped.slice(pos).match(WHEN_TOKEN_RE);
    if (m) {
      tokens.push(m[1]);
      pos += m[1].length;
    } else {
      return { bad: stripped.slice(pos, pos + 4) };
    }
  }
  return tokens;
}

/**
 * Parse `when` as a boolean expression; return a problem string on invalid syntax.
 *
 * Grammar:
 *   expr    ::= or
 *   or      ::= and ('||' and)*
 *   and     ::= not ('&&' not)*
 *   not     ::= '!' not | primary
 *   primary ::= ID | '(' expr ')'
 */
function whenSyntaxProblems(when: string, index: number): string[] {
  const prefix = `entries[${index}].when`;
  const stripped = when.trim();

  const toks = tokenizeWhen(stripped);
  if (!Array.isArray(toks)) {
    return [`${prefix}: unexpected token "${toks.bad}"`];
  }
  if (toks.length === 0) {
    return [`${prefix}: expression cannot be empty`];
  }

  const tokens = toks;
  let pos = 0;

  const peek = (): string | undefined => tokens[pos];
  const consume = (): string => tokens[pos++];

  const parseOr = (): void => { parseAnd(); while (peek() === "||") { consume(); parseAnd(); } };
  const parseAnd = (): void => { parseNot(); while (peek() === "&&") { consume(); parseNot(); } };
  const parseNot = (): void => { if (peek() === "!") { consume(); parseNot(); } else parsePrimary(); };
  const parsePrimary = (): void => {
    const t = peek();
    if (t === undefined) throw new Error("unexpected end of expression");
    if (t === "(") {
      consume();
      parseOr();
      if (peek() !== ")") throw new Error("expected ')'");
      consume();
    } else if (/^[a-z][a-z0-9_]{0,31}$/.test(t)) {
      consume();
    } else {
      throw new Error(`unexpected token "${t}"`);
    }
  };

  try {
    parseOr();
    if (pos < tokens.length) throw new Error(`unexpected token "${tokens[pos]}"`);
  } catch (e) {
    return [`${prefix}: ${(e as Error).message}`];
  }

  return [];
}

/**
 * Beyond JSON-Schema: enforce semantic invariants the schema cannot express.
 * Returns the list of human-readable problems (empty = ok).
 */
export function semanticCheck(spec: StrategySpec): string[] {
  const problems: string[] = [];

  const indicatorIds = new Set(spec.indicators.map((i) => i.id));
  const patternIds = new Set(spec.patterns.map((p) => p.id));
  const knownIds = new Set([...indicatorIds, ...patternIds]);
  if (indicatorIds.size !== spec.indicators.length) problems.push("indicator ids must be unique");
  if (patternIds.size !== spec.patterns.length) problems.push("pattern ids must be unique");

  const idRe = /[a-z][a-z0-9_]{0,31}/g;
  for (const [i, e] of spec.entries.entries()) {
    problems.push(...whenSyntaxProblems(e.when, i));
    const refs = e.when.match(idRe) ?? [];
    for (const r of refs) {
      if (r === "true" || r === "false") continue;
      if (!knownIds.has(r)) problems.push(`entries[${i}].when references unknown id "${r}"`);
    }
  }

  if (spec.risk.min_rr && Number(spec.risk.min_rr) < 3) {
    problems.push("risk.min_rr must be >= 3 (Apostila methodology requires >= 3)");
  }

  return problems;
}
