import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { validate, hash, canonicalize, semanticCheck, withHash } from "./index.js";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.resolve(__dirname, "../../fixtures/wyckoff_spring_btc_15m.json");
const fixture = JSON.parse(readFileSync(FIXTURE, "utf8"));

test("golden fixture validates", () => {
  const r = validate(fixture);
  assert.equal(r.ok, true);
});

test("canonicalize is deterministic regardless of key order", () => {
  const a = { b: 1, a: 2, c: { y: 1, x: 2 } };
  const b = { c: { x: 2, y: 1 }, a: 2, b: 1 };
  assert.equal(canonicalize(a), canonicalize(b));
});

test("canonicalize preserves non-ASCII as UTF-8 (not \\uXXXX)", () => {
  const result = canonicalize({ quote: "ação" });
  assert.equal(result, '{"quote":"ação"}');
  assert.ok(!result.includes("\\u"), "must not escape non-ASCII chars");
});

test("hash is stable for the golden fixture", () => {
  const h1 = hash(fixture);
  const h2 = hash(fixture);
  assert.equal(h1, h2);
  assert.match(h1, /^[0-9a-f]{64}$/);
});

test("hash matches pinned cross-language value", () => {
  const pinned = readFileSync(
    path.resolve(__dirname, "../../fixtures/wyckoff_spring_btc_15m.hash.txt"),
    "utf8",
  ).trim();
  assert.equal(hash(fixture), pinned);
});

test("withHash sets a valid spec_hash", () => {
  const sealed = withHash(fixture);
  assert.equal(sealed.spec_hash, hash(fixture));
});

test("semanticCheck passes on golden", () => {
  assert.deepEqual(semanticCheck(fixture), []);
});

test("semanticCheck flags unknown id in entry condition", () => {
  const broken = JSON.parse(JSON.stringify(fixture));
  broken.entries[0].when = "wy_spring && does_not_exist";
  const problems = semanticCheck(broken);
  assert.ok(problems.some((p) => p.includes("does_not_exist")));
});

test("semanticCheck rejects dangling && operator", () => {
  const broken = JSON.parse(JSON.stringify(fixture));
  broken.entries[0].when = "wy_spring &&";
  const problems = semanticCheck(broken);
  assert.ok(problems.some((p) => p.includes("trailing binary operator")));
});

test("semanticCheck rejects function call in when", () => {
  const broken = JSON.parse(JSON.stringify(fixture));
  broken.entries[0].when = "wy_spring()";
  const problems = semanticCheck(broken);
  assert.ok(problems.some((p) => p.includes("function call")));
});

test("semanticCheck rejects arithmetic operator in when", () => {
  const broken = JSON.parse(JSON.stringify(fixture));
  broken.entries[0].when = "wy_spring + vsa_no_supply";
  const problems = semanticCheck(broken);
  assert.ok(problems.some((p) => p.includes("arithmetic")));
});

test("semanticCheck rejects min_rr below 3", () => {
  const broken = JSON.parse(JSON.stringify(fixture));
  broken.risk.min_rr = "1.5";
  const problems = semanticCheck(broken);
  assert.ok(problems.some((p) => p.includes("min_rr")));
});

test("validation rejects float per_trade_pct", () => {
  const broken = JSON.parse(JSON.stringify(fixture));
  broken.risk.per_trade_pct = 0.5; // float, not decimal-string
  const r = validate(broken);
  assert.equal(r.ok, false);
});

test("validation rejects invalid uuid pdf_id", () => {
  const broken = JSON.parse(JSON.stringify(fixture));
  broken.citations[0].pdf_id = "not-a-uuid";
  const r = validate(broken);
  assert.equal(r.ok, false);
});
