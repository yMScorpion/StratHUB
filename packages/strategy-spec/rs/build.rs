use std::path::Path;

fn main() {
    let schema_src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("schema")
        .join("strategy-spec.schema.json")
        .canonicalize()
        .expect("schema path must exist");
    println!("cargo:rerun-if-changed={}", schema_src.display());
    println!(
        "cargo:rustc-env=STRATEGY_SPEC_SCHEMA_PATH={}",
        schema_src.display()
    );
}
