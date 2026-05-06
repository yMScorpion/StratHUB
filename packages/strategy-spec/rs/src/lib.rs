//! Canonical Strategy Spec — Rust binding.
//!
//! - [`StrategySpec`] is the typed view (serde) of the JSON spec.
//! - [`validate`] performs full JSON-Schema validation against the bundled schema.
//! - [`canonicalize`] / [`hash_spec`] / [`with_hash`] are byte-compatible with the TS and Python
//!   implementations.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use jsonschema::Validator;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

const SCHEMA_JSON: &str = include_str!(env!("STRATEGY_SPEC_SCHEMA_PATH"));

#[derive(Debug, Error)]
pub enum SpecError {
    #[error("schema validation failed: {0}")]
    Schema(String),
    #[error("semantic validation failed: {0:?}")]
    Semantic(Vec<SemanticProblem>),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticProblem {
    pub code: &'static str,
    pub message: String,
    pub path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum AssetClass {
    Spot,
    Perp,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Exchange {
    Binance,
    Bybit,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct StrategySpec {
    pub spec_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_hash: Option<String>,
    pub asset_class: AssetClass,
    pub exchange: Exchange,
    pub symbols: Vec<String>,
    pub timeframe: String,
    pub indicators: Vec<Value>,
    pub patterns: Vec<Value>,
    pub filters: Vec<Value>,
    pub entries: Vec<Value>,
    pub exits: Vec<Value>,
    pub risk: Value,
    pub citations: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, String>>,
}

fn validator() -> &'static Validator {
    static V: OnceLock<Validator> = OnceLock::new();
    V.get_or_init(|| {
        let schema: Value = serde_json::from_str(SCHEMA_JSON).expect("schema is valid JSON");
        Validator::options()
            .build(&schema)
            .expect("schema compiles")
    })
}

/// Validate `spec` against the JSON Schema. Returns `Ok` on success.
pub fn validate(spec: &Value) -> Result<(), SpecError> {
    let v = validator();
    let errors: Vec<String> = v.iter_errors(spec).map(|e| e.to_string()).collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(SpecError::Schema(errors.join("; ")))
    }
}

/// Canonical JSON. Must match the TS and Python implementations byte-for-byte:
/// recursively sorted object keys, no whitespace, UTF-8 strings (no \uXXXX escaping),
/// positive scientific notation exponents normalised to include `+` (1e21 → 1e+21),
/// and integer-valued floats normalised to ints (matches JS Number semantics).
pub fn canonicalize(value: &Value) -> String {
    let normalized = normalize(value);
    let raw = serde_json::to_string(&normalized)
        .expect("serialization is infallible for normalized Value");
    // serde_json/ryu omits the `+` sign on positive exponents (e.g. `1e21`),
    // but both Python json.dumps and JS JSON.stringify emit `1e+21`. Fix that here.
    fix_positive_exponents(raw)
}

/// Add `+` to bare positive scientific-notation exponents outside of JSON strings.
/// serde_json uses ryu which outputs `1e21`; Python/JS output `1e+21`.
fn fix_positive_exponents(s: String) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut in_str = false;
    let mut escaped = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if escaped {
            escaped = false;
            out.push(c);
            continue;
        }
        if c == '\\' && in_str {
            escaped = true;
            out.push(c);
            continue;
        }
        if c == '"' {
            in_str = !in_str;
            out.push(c);
            continue;
        }
        if c == 'e' && !in_str {
            out.push(c);
            if matches!(chars.peek(), Some('0'..='9')) {
                out.push('+');
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn normalize(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), normalize(&map[k]));
            }
            Value::Object(sorted)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(normalize).collect()),
        Value::Number(n) => normalize_number(n.clone()),
        other => other.clone(),
    }
}

fn normalize_number(n: serde_json::Number) -> Value {
    if n.is_f64() {
        if let Some(f) = n.as_f64() {
            if f.is_finite() && f.fract() == 0.0 && f.abs() < (i64::MAX as f64) {
                return Value::Number((f as i64).into());
            }
        }
    }
    Value::Number(n)
}

/// sha256 of the canonical JSON of `spec` with `spec_hash` removed.
pub fn hash_spec(spec: &Value) -> String {
    let mut body = spec.clone();
    if let Some(obj) = body.as_object_mut() {
        obj.remove("spec_hash");
    }
    let canonical = canonicalize(&body);
    let mut h = Sha256::new();
    h.update(canonical.as_bytes());
    hex::encode(h.finalize())
}

/// Insert `spec_hash` into `spec` and return the result.
pub fn with_hash(spec: &Value) -> Value {
    let mut out = spec.clone();
    if let Some(obj) = out.as_object_mut() {
        obj.insert("spec_hash".to_string(), Value::String(hash_spec(spec)));
    }
    out
}

/// Beyond JSON-Schema invariants. Returns `Ok(())` on success.
pub fn semantic_check(spec: &Value) -> Result<(), SpecError> {
    let mut problems = Vec::new();

    let indicators = spec.get("indicators").and_then(|v| v.as_array());
    let patterns = spec.get("patterns").and_then(|v| v.as_array());

    let id_of = |arr: &[Value]| -> Vec<String> {
        arr.iter()
            .filter_map(|v| v.get("id").and_then(|s| s.as_str()).map(String::from))
            .collect()
    };

    let ind_ids: Vec<String> = indicators.map(|a| id_of(a)).unwrap_or_default();
    let pat_ids: Vec<String> = patterns.map(|a| id_of(a)).unwrap_or_default();

    if has_dups(&ind_ids) {
        problems.push(SemanticProblem {
            code: "dup_indicator_id",
            message: "indicator ids must be unique".into(),
            path: Some("indicators".into()),
        });
    }
    if has_dups(&pat_ids) {
        problems.push(SemanticProblem {
            code: "dup_pattern_id",
            message: "pattern ids must be unique".into(),
            path: Some("patterns".into()),
        });
    }

    let mut known: std::collections::HashSet<&str> = std::collections::HashSet::new();
    known.extend(ind_ids.iter().map(String::as_str));
    known.extend(pat_ids.iter().map(String::as_str));

    if let Some(entries) = spec.get("entries").and_then(|v| v.as_array()) {
        for (i, entry) in entries.iter().enumerate() {
            let when = entry.get("when").and_then(|v| v.as_str()).unwrap_or("");
            check_when_syntax(when, i, &mut problems);
            for tok in tokens(when) {
                if tok == "true" || tok == "false" {
                    continue;
                }
                if !known.contains(tok.as_str()) {
                    problems.push(SemanticProblem {
                        code: "unknown_id",
                        message: format!("entries[{i}].when references unknown id \"{tok}\""),
                        path: Some(format!("entries[{i}].when")),
                    });
                }
            }
        }
    }

    if let Some(min_rr) = spec
        .get("risk")
        .and_then(|r| r.get("min_rr"))
        .and_then(|v| v.as_str())
    {
        if let Ok(n) = min_rr.parse::<f64>() {
            if n < 3.0 {
                problems.push(SemanticProblem {
                    code: "min_rr_below_three",
                    message: "risk.min_rr must be >= 3 (Apostila methodology requires >= 3)".into(),
                    path: Some("risk.min_rr".into()),
                });
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(SpecError::Semantic(problems))
    }
}

/// Validate the boolean `when` expression syntax using a recursive descent parser.
///
/// Grammar:
///   expr    ::= or
///   or      ::= and ('||' and)*
///   and     ::= not ('&&' not)*
///   not     ::= '!' not | primary
///   primary ::= ID | '(' expr ')'
fn check_when_syntax(when: &str, index: usize, problems: &mut Vec<SemanticProblem>) {
    let path = format!("entries[{index}].when");
    match parse_when(when.trim()) {
        Ok(()) => {}
        Err(msg) => problems.push(SemanticProblem {
            code: "invalid_when_syntax",
            message: format!("entries[{index}].when: {msg}"),
            path: Some(path),
        }),
    }
}

fn parse_when(s: &str) -> Result<(), String> {
    if s.is_empty() {
        return Err("expression cannot be empty".into());
    }
    let tokens = tokenize_when(s)?;
    let mut pos = 0usize;
    parse_or(&tokens, &mut pos)?;
    if pos < tokens.len() {
        return Err(format!("unexpected token {:?}", tokens[pos]));
    }
    Ok(())
}

fn tokenize_when(s: &str) -> Result<Vec<String>, String> {
    let mut tokens: Vec<String> = Vec::new();
    let b = s.as_bytes();
    let n = b.len();
    let mut i = 0;
    while i < n {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if n - i >= 2 && &b[i..i + 2] == b"&&" {
            tokens.push("&&".into());
            i += 2;
        } else if n - i >= 2 && &b[i..i + 2] == b"||" {
            tokens.push("||".into());
            i += 2;
        } else if b[i] == b'!' {
            tokens.push("!".into());
            i += 1;
        } else if b[i] == b'(' {
            tokens.push("(".into());
            i += 1;
        } else if b[i] == b')' {
            tokens.push(")".into());
            i += 1;
        } else if b[i].is_ascii_lowercase() {
            let start = i;
            i += 1;
            while i < n && (b[i].is_ascii_lowercase() || b[i].is_ascii_digit() || b[i] == b'_') {
                i += 1;
            }
            tokens.push(s[start..i].to_string());
        } else {
            let end = std::cmp::min(i + 4, n);
            return Err(format!("unexpected token {:?}", &s[i..end]));
        }
    }
    Ok(tokens)
}

fn parse_or(tokens: &[String], pos: &mut usize) -> Result<(), String> {
    parse_and(tokens, pos)?;
    while tokens.get(*pos).map(String::as_str) == Some("||") {
        *pos += 1;
        parse_and(tokens, pos)?;
    }
    Ok(())
}

fn parse_and(tokens: &[String], pos: &mut usize) -> Result<(), String> {
    parse_not(tokens, pos)?;
    while tokens.get(*pos).map(String::as_str) == Some("&&") {
        *pos += 1;
        parse_not(tokens, pos)?;
    }
    Ok(())
}

fn parse_not(tokens: &[String], pos: &mut usize) -> Result<(), String> {
    if tokens.get(*pos).map(String::as_str) == Some("!") {
        *pos += 1;
        parse_not(tokens, pos)
    } else {
        parse_primary(tokens, pos)
    }
}

fn parse_primary(tokens: &[String], pos: &mut usize) -> Result<(), String> {
    match tokens.get(*pos) {
        None => Err("unexpected end of expression".into()),
        Some(t) if t == "(" => {
            *pos += 1;
            parse_or(tokens, pos)?;
            if tokens.get(*pos).map(String::as_str) != Some(")") {
                return Err("expected ')'".into());
            }
            *pos += 1;
            Ok(())
        }
        Some(t) if t.starts_with(|c: char| c.is_ascii_lowercase()) => {
            *pos += 1;
            Ok(())
        }
        Some(t) => Err(format!("unexpected token {:?}", t)),
    }
}

fn has_dups(xs: &[String]) -> bool {
    let mut seen = std::collections::HashSet::with_capacity(xs.len());
    xs.iter().any(|x| !seen.insert(x.as_str()))
}

fn tokens(when: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in when.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
            cur.push(c);
        } else if !cur.is_empty() && cur.starts_with(|x: char| x.is_ascii_lowercase()) {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.clear();
        }
    }
    if !cur.is_empty() && cur.starts_with(|x: char| x.is_ascii_lowercase()) {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("fixtures")
            .join("wyckoff_spring_btc_15m.json");
        let bytes = std::fs::read(&path).expect("fixture present");
        serde_json::from_slice(&bytes).expect("fixture is valid JSON")
    }

    #[test]
    fn golden_validates() {
        let v = fixture();
        validate(&v).expect("schema valid");
        semantic_check(&v).expect("semantic valid");
    }

    #[test]
    fn canonicalize_is_key_order_invariant() {
        let a = json!({"b": 1, "a": 2, "c": {"y": 1, "x": 2}});
        let b = json!({"c": {"x": 2, "y": 1}, "a": 2, "b": 1});
        assert_eq!(canonicalize(&a), canonicalize(&b));
    }

    #[test]
    fn canonicalize_non_ascii_utf8_passthrough() {
        // Non-ASCII must NOT be escaped as \uXXXX — must stay UTF-8 to match JS/Python.
        let v = json!({"quote": "ação"});
        assert_eq!(canonicalize(&v), r#"{"quote":"ação"}"#);
    }

    #[test]
    fn canonicalize_positive_exponent_normalised() {
        // serde_json/ryu emits 1e21 but Python/JS emit 1e+21.
        let v = json!({"n": 1e21_f64});
        let s = canonicalize(&v);
        assert!(
            s.contains("e+"),
            "expected e+ in canonical output, got: {s}"
        );
    }

    #[test]
    fn hash_is_stable() {
        let v = fixture();
        let h1 = hash_spec(&v);
        let h2 = hash_spec(&v);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
        assert!(h1
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn hash_matches_pinned_cross_language_value() {
        // This is the same byte string asserted by the Python and TS test suites.
        // Drift here means a canonicalization or hashing divergence between languages.
        let pinned = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("fixtures")
                .join("wyckoff_spring_btc_15m.hash.txt"),
        )
        .expect("hash fixture present");
        assert_eq!(hash_spec(&fixture()), pinned.trim());
    }

    #[test]
    fn with_hash_round_trips() {
        let v = fixture();
        let sealed = with_hash(&v);
        assert_eq!(
            sealed.get("spec_hash").and_then(|v| v.as_str()).unwrap(),
            hash_spec(&v),
        );
    }

    #[test]
    fn semantic_flags_unknown_id() {
        let mut v = fixture();
        v["entries"][0]["when"] = json!("wy_spring && does_not_exist");
        let err = semantic_check(&v).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("does_not_exist"), "got: {msg}");
    }

    #[test]
    fn semantic_rejects_dangling_and_operator() {
        let mut v = fixture();
        v["entries"][0]["when"] = json!("wy_spring &&");
        assert!(semantic_check(&v).is_err());
    }

    #[test]
    fn semantic_rejects_function_call_in_when() {
        let mut v = fixture();
        v["entries"][0]["when"] = json!("wy_spring()");
        assert!(semantic_check(&v).is_err());
    }

    #[test]
    fn semantic_rejects_arithmetic_in_when() {
        let mut v = fixture();
        v["entries"][0]["when"] = json!("wy_spring + vsa_no_supply");
        assert!(semantic_check(&v).is_err());
    }

    #[test]
    fn semantic_rejects_min_rr_below_three() {
        let mut v = fixture();
        v["risk"]["min_rr"] = json!("1.5");
        let err = semantic_check(&v).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("min_rr"), "got: {msg}");
    }

    #[test]
    fn validation_rejects_float_per_trade_pct() {
        let mut v = fixture();
        v["risk"]["per_trade_pct"] = json!(0.5);
        assert!(validate(&v).is_err());
    }
}
