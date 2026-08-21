use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const PASSING_MODEL: &str = r#"{
  "$schema": "des/state-machine/v1",
  "name": "lease",
  "initial": "free",
  "states": {
    "free": {"owners": 0},
    "held": {"owners": 1},
    "released": {"owners": 0}
  },
  "transitions": [
    {"event": "acquire", "from": "free", "to": "held"},
    {"event": "release", "from": "held", "to": "released"}
  ],
  "invariants": [
    {
      "name": "at most one owner",
      "assert": [
        {"path": "/owners", "op": "lte", "right": {"value": 1}}
      ]
    }
  ],
  "terminal_states": ["released"]
}"#;

fn temp_model(tag: &str, contents: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "des-formal-check-{tag}-{}-{nonce}.json",
        std::process::id()
    ));
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn cli_passes_a_valid_model_and_fails_a_counterexample() {
    let passing = temp_model("pass", PASSING_MODEL);
    let pass_output = Command::new(env!("CARGO_BIN_EXE_des-formal-check"))
        .arg(&passing)
        .output()
        .unwrap();
    assert!(pass_output.status.success());
    assert!(String::from_utf8_lossy(&pass_output.stdout).contains("— PASS"));

    let failing_raw =
        PASSING_MODEL.replace("\"held\": {\"owners\": 1}", "\"held\": {\"owners\": 2}");
    let failing = temp_model("fail", &failing_raw);
    let fail_output = Command::new(env!("CARGO_BIN_EXE_des-formal-check"))
        .arg(&failing)
        .output()
        .unwrap();
    assert_eq!(fail_output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&fail_output.stdout);
    assert!(stdout.contains("— FAIL"));
    assert!(stdout.contains("Shortest counterexample trace"));

    std::fs::remove_file(passing).unwrap();
    std::fs::remove_file(failing).unwrap();
}

#[test]
fn cli_distinguishes_invalid_input_from_a_failed_proof_obligation() {
    let invalid = temp_model("invalid", "{not-json}");
    let output = Command::new(env!("CARGO_BIN_EXE_des-formal-check"))
        .arg(&invalid)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid model JSON"));
    std::fs::remove_file(invalid).unwrap();
}
