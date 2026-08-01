//! DES-unique sim-model spec inspector. Parses one of the org's JSON model
//! specs (des-engine's `examples/*.json`, ~77 distinct `model` citizens, or a
//! discrete-event-system.rs `data/*.json`) and reports its structure without
//! running anything: the spec schema, the model/citizen it drives, parameters,
//! runtime knobs, and — for the "universal math" DES documents — the math
//! payload (state variables, equations, block graph). Read-only, parse-only.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Cap on the spec file we will read into memory (specs are tiny; the biggest
/// in the corpus is a few hundred KB). Keeps a stray huge file from OOMing.
pub const MAX_SPEC_BYTES: u64 = 4_000_000;

/// Resolve a spec file under a repo, defending against path traversal: the
/// caller-supplied `rel` must stay inside `repo_dir` and be an existing file.
pub fn resolve_spec(repo_dir: &Path, rel: &str) -> Result<PathBuf, String> {
    if rel.is_empty() || rel.starts_with('/') || rel.contains("..") || rel.contains('\\') {
        return Err(format!(
            "invalid spec path {rel:?} (no absolute paths, no `..`)"
        ));
    }
    // Only JSON specs are inspectable.
    if !rel.ends_with(".json") {
        return Err(format!("{rel:?} is not a .json spec"));
    }
    let joined = repo_dir.join(rel);
    // Belt-and-suspenders: the resolved path must remain under the repo dir.
    let canon_repo = repo_dir
        .canonicalize()
        .map_err(|e| format!("cannot resolve repo dir: {e}"))?;
    let canon = joined
        .canonicalize()
        .map_err(|_| format!("no such spec file {rel:?} under {}", repo_dir.display()))?;
    if !canon.starts_with(&canon_repo) {
        return Err(format!("spec path {rel:?} escapes the repo directory"));
    }
    if !canon.is_file() {
        return Err(format!("{rel:?} is not a file"));
    }
    Ok(canon)
}

fn arr_len(v: &Value, key: &str) -> Option<usize> {
    v.get(key).and_then(Value::as_array).map(Vec::len)
}

fn names(v: &Value, key: &str, id_field: &str, limit: usize) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|e| {
            e.get(id_field)
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| e.as_str().map(str::to_string))
        })
        .take(limit)
        .collect()
}

/// Parse and summarize a model spec. `label` is a human-facing name for the
/// spec (e.g. its path) used only in the header.
pub fn inspect(label: &str, raw: &str) -> Result<String, String> {
    let v: Value =
        serde_json::from_str(raw).map_err(|e| format!("{label}: not valid JSON ({e})"))?;
    let obj = v
        .as_object()
        .ok_or_else(|| format!("{label}: top-level JSON is not an object"))?;

    let mut out = format!("# model spec: {label}\n\n");

    let get_str = |k: &str| v.get(k).and_then(Value::as_str);
    if let Some(schema) = get_str("$schema") {
        out.push_str(&format!("schema:      {schema}\n"));
    }
    // `model` (des-engine citizen name) or `id` (universal-math document id).
    if let Some(model) = get_str("model") {
        out.push_str(&format!(
            "model:       {model}   (the citizen/solver this spec drives)\n"
        ));
    } else if let Some(id) = get_str("id") {
        out.push_str(&format!("id:          {id}\n"));
    }
    if let Some(desc) = get_str("description") {
        // Char-boundary-safe: a multi-byte glyph at byte 240 would panic a
        // raw `&desc[..240]`, so a unicode description crashes the tool call.
        out.push_str(&format!(
            "description: {}\n",
            crate::util::truncate_field(desc, 240)
        ));
    }

    // Top-level keys, so nothing is hidden even for unusual specs.
    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    out.push_str(&format!("top-level keys: {}\n\n", keys.join(", ")));

    // Parameters (a flat object of knobs in the common spec shape).
    if let Some(params) = v.get("parameters") {
        match params {
            Value::Object(m) => {
                out.push_str(&format!("## parameters ({} knob(s))\n", m.len()));
                for (k, val) in m.iter().take(40) {
                    out.push_str(&format!("  {k} = {}\n", compact(val)));
                }
                if m.len() > 40 {
                    out.push_str(&format!("  …[{} more]\n", m.len() - 40));
                }
                out.push('\n');
            }
            Value::Array(a) => {
                // universal-math shape: parameters is a list of {id,value}.
                out.push_str(&format!("## parameters ({} entry/entries)\n", a.len()));
                for name in names(&v, "parameters", "id", 40) {
                    out.push_str(&format!("  {name}\n"));
                }
                out.push('\n');
            }
            _ => {}
        }
    }

    if let Some(runtime) = v.get("runtime").and_then(Value::as_object) {
        let ks: Vec<&str> = runtime.keys().map(String::as_str).collect();
        out.push_str(&format!("runtime knobs: {}\n", ks.join(", ")));
    }
    if let Some(outputs) = v.get("outputs") {
        out.push_str(&format!("outputs: {}\n", compact(outputs)));
    }
    if let Some(tags) = v.pointer("/metadata/tags").and_then(Value::as_array) {
        let t: Vec<&str> = tags.iter().filter_map(Value::as_str).collect();
        if !t.is_empty() {
            out.push_str(&format!("tags: {}\n", t.join(", ")));
        }
    }

    // Universal-math DES document: report the math payload as
    // entities/events-of-record for a discrete-event SYSTEM.
    if let Some(math) = v.get("math") {
        out.push('\n');
        out.push_str("## math payload (universal DES document)\n");
        if let Some(kind) = math.get("kind").and_then(Value::as_str) {
            out.push_str(&format!("  kind: {kind}\n"));
        }
        for (label, key) in [
            ("state variables", "stateVariables"),
            ("independent variables", "independentVariables"),
            ("parameters", "parameters"),
            ("equations", "equations"),
            ("initial conditions", "initialConditions"),
        ] {
            if let Some(n) = arr_len(math, key) {
                let ns = names(math, key, "id", 8);
                out.push_str(&format!(
                    "  {label}: {n}{}\n",
                    if ns.is_empty() {
                        String::new()
                    } else {
                        format!("  [{}]", ns.join(", "))
                    }
                ));
            }
        }
    }

    // Generic entity/station/event graph, if a spec ever carries one inline.
    for (label, key) in [
        ("entities", "entities"),
        ("stations", "stations"),
        ("events", "events"),
        ("blocks", "blocks"),
    ] {
        if let Some(n) = arr_len(&v, key) {
            out.push_str(&format!("{label}: {n}\n"));
        }
    }

    Ok(out)
}

/// Render a JSON value in a short, single-line form for parameter listings.
fn compact(v: &Value) -> String {
    let s = match v {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    // Char-boundary-safe: parameter values come straight from the spec JSON,
    // so a multi-byte glyph at byte 100 must not panic a raw `&s[..100]`.
    crate::util::truncate_field(&s, 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_reports_common_spec_shape() {
        let raw = r#"{
            "$schema": "des/model-spec/v1",
            "model": "blackjack-mc",
            "description": "Blackjack with first-visit Monte Carlo control",
            "parameters": {"numEpisodes": 200000, "epsilon": 0.1, "gamma": 1.0},
            "runtime": {"seed": 7},
            "metadata": {"tags": ["rl", "monte-carlo"]}
        }"#;
        let out = inspect("blackjack-mc.json", raw).unwrap();
        assert!(out.contains("schema:      des/model-spec/v1"));
        assert!(out.contains("model:       blackjack-mc"));
        assert!(out.contains("## parameters (3 knob(s))"));
        assert!(out.contains("numEpisodes = 200000"));
        assert!(out.contains("runtime knobs: seed"));
        assert!(out.contains("tags: rl, monte-carlo"));
    }

    #[test]
    fn inspect_reports_universal_math_payload() {
        let raw = r#"{
            "$schema": "des/universal-model/v1",
            "id": "universal-latex-decay-ode",
            "description": "y' = -k y",
            "math": {
                "kind": "ode",
                "stateVariables": [{"id": "y", "role": "state"}],
                "parameters": [{"id": "k", "value": 1}],
                "equations": [{"id": "ode:y", "kind": "ode"}]
            }
        }"#;
        let out = inspect("ode.json", raw).unwrap();
        assert!(out.contains("id:          universal-latex-decay-ode"));
        assert!(out.contains("## math payload"));
        assert!(out.contains("kind: ode"));
        assert!(out.contains("state variables: 1  [y]"));
        assert!(out.contains("equations: 1  [ode:y]"));
    }

    #[test]
    fn inspect_survives_multibyte_description_and_params() {
        // A description and a parameter value longer than the inline caps and
        // built from multi-byte chars would panic the old `&s[..N]` slices at
        // a non-char-boundary. Inspect must truncate safely, never crash.
        let desc = "€".repeat(400); // 1200 bytes, cut at 240 lands mid-char
        let pval = "λ".repeat(120); // 240 bytes, cut at 100 lands mid-char
        let raw = format!(
            r#"{{"$schema":"des/model-spec/v1","model":"unicode-mc",
                "description":"{desc}","parameters":{{"greek":"{pval}"}}}}"#
        );
        let out = inspect("unicode.json", &raw).unwrap();
        assert!(out.contains("model:       unicode-mc"));
        assert!(out.contains("description: €"));
        assert!(out.contains("greek = λ"));
        // truncation markers present, and the output is valid UTF-8 by virtue
        // of being a String (a bad slice would have panicked before here).
        assert!(out.contains('…'));
    }

    #[test]
    fn inspect_rejects_non_json_and_non_object() {
        assert!(inspect("x", "not json").is_err());
        assert!(inspect("x", "[1,2,3]").is_err());
    }

    #[test]
    fn resolve_spec_rejects_traversal_and_non_json() {
        let dir = std::env::temp_dir();
        for bad in [
            "../etc/passwd",
            "/etc/passwd",
            "a/../../b.json",
            "notes.md",
            "",
        ] {
            assert!(resolve_spec(&dir, bad).is_err(), "should reject {bad:?}");
        }
    }
}
