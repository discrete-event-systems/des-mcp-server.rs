//! Regression and independent-oracle tests for the verifier itself.
use std::collections::BTreeSet;

use des_mcp_server::formal::{DEFAULT_MAX_STATES, check_json, check_json_strict};
use serde_json::{Value, json};

fn comparison(left: &str, right: &str, op: &str) -> String {
    format!(
        r#"{{
          "$schema": "des/state-machine/v1",
          "name": "numeric regression",
          "initial": "done",
          "states": {{"done": {{"left": {left}, "right": {right}}}}},
          "terminal_states": ["done"],
          "invariants": [{{"name": "comparison", "assert": [{{
            "path": "/left", "op": "{op}", "right": {{"path": "/right"}}
          }}]}}]
        }}"#
    )
}

#[test]
fn mixed_numeric_comparisons_do_not_round_integers_into_false_passes() {
    for (left, right, op, expected) in [
        ("9007199254740993", "9007199254740992.0", "lte", false),
        ("9007199254740993", "9007199254740992.0", "gt", true),
        (
            "18446744073709551615",
            "18446744073709551616.0",
            "gte",
            false,
        ),
        ("18446744073709551616.0", "18446744073709551615", "gt", true),
        ("-9007199254740993", "-9007199254740992.0", "gte", false),
        ("-9223372036854775808", "18446744073709551615", "lt", true),
        ("0", "0.5", "lt", true),
        ("0", "-0.5", "gt", true),
        ("1", "1.0", "eq", true),
        ("1", "1.0", "ne", false),
        ("0", "-0.0", "eq", true),
    ] {
        let raw = comparison(left, right, op);
        let report = check_json(&raw, DEFAULT_MAX_STATES).unwrap();
        assert_eq!(report.passed(), expected, "{left} {op} {right}");
    }
}

#[test]
fn duplicate_state_names_and_nested_keys_are_rejected() {
    let cases = [
        r#"{"done":{"left":2},"done":{"left":0}}"#,
        r#"{"done":{"left":2,"left":0}}"#,
        r#"{"done":{"left":2,"\u006ceft":0}}"#,
    ];
    for states in cases {
        let raw = format!(
            r#"{{"$schema":"des/state-machine/v1","name":"duplicate",
            "initial":"done","states":{states},"terminal_states":["done"]}}"#
        );
        assert!(check_json(&raw, DEFAULT_MAX_STATES).is_err(), "{raw}");
    }
}

#[test]
fn out_of_range_integer_literals_are_not_silently_treated_as_floats() {
    for integer in ["18446744073709551616", "-9223372036854775809"] {
        let raw = comparison(integer, "0", "gte");
        assert!(check_json(&raw, DEFAULT_MAX_STATES).is_err(), "{integer}");
    }
}

#[test]
fn missing_literal_operand_is_not_implicitly_null() {
    let raw = comparison("null", "null", "eq").replace(r#"{"path": "/right"}"#, "{}");
    assert!(check_json(&raw, DEFAULT_MAX_STATES).is_err());
}

#[test]
fn missing_comparison_guard_cannot_make_an_invariant_vacuously_pass() {
    let raw = json!({
        "$schema": "des/state-machine/v1",
        "name": "missing guard",
        "initial": "done",
        "states": {
            "done": {"amount": -1},
            "unreachable": {"kind": "protected", "amount": 1}
        },
        "terminal_states": ["done"],
        "invariants": [{
            "name": "nonnegative protected amount",
            "when": [{"path": "/kind", "op": "eq", "right": {"value": "protected"}}],
            "assert": [{"path": "/amount", "op": "gte", "right": {"value": 0}}]
        }]
    });
    let report = check_json(&raw.to_string(), DEFAULT_MAX_STATES).unwrap();
    assert!(!report.passed());
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.code == "invalid-invariant-predicate")
    );
}

#[test]
fn an_explicit_presence_guard_can_skip_an_optional_field() {
    let raw = json!({
        "$schema": "des/state-machine/v1",
        "name": "optional guard",
        "initial": "done",
        "states": {"done": {}, "unreachable": {"amount": 1}},
        "terminal_states": ["done"],
        "invariants": [{
            "name": "optional amount",
            "when": [{"path": "/amount", "op": "exists"}],
            "assert": [{"path": "/amount", "op": "gte", "right": {"value": 0}}]
        }]
    });
    let report = check_json(&raw.to_string(), DEFAULT_MAX_STATES).unwrap();
    assert!(report.passed());
    assert!(report.warnings.iter().any(|w| w.contains("never matched")));
}

#[test]
fn the_library_enforces_the_input_byte_limit() {
    let raw = format!("{}{}", comparison("0", "0", "eq"), " ".repeat(4_000_001));
    assert!(check_json(&raw, DEFAULT_MAX_STATES).is_err());
}

fn targets(report: &des_mcp_server::formal::CheckReport, code: &str) -> BTreeSet<String> {
    report
        .violations
        .iter()
        .filter(|violation| violation.code == code)
        .map(|violation| violation.trace.last().unwrap().state.clone())
        .collect()
}

#[test]
fn every_three_state_graph_agrees_with_an_independent_transitive_closure_oracle() {
    // 2^(3*3) directed graphs, including self-loops. The production checker
    // uses BFS/reverse BFS; this oracle uses relation composition instead.
    for mask in 0u16..512 {
        let edges: BTreeSet<(usize, usize)> = (0..9)
            .filter(|bit| mask & (1 << bit) != 0)
            .map(|bit| (bit / 3, bit % 3))
            .collect();
        let mut reach: BTreeSet<(usize, usize)> = edges.clone();
        reach.extend((0..3).map(|state| (state, state)));
        for middle in 0..3 {
            let before = reach.clone();
            for from in 0..3 {
                for to in 0..3 {
                    if before.contains(&(from, middle)) && before.contains(&(middle, to)) {
                        reach.insert((from, to));
                    }
                }
            }
        }
        let transitions: Vec<Value> = edges
            .iter()
            .map(|(from, to)| {
                json!({"from": format!("s{from}"), "to": format!("s{to}"),
                    "event": format!("e{from}-{to}")})
            })
            .collect();
        let raw = json!({
            "$schema": "des/state-machine/v1",
            "name": format!("graph {mask}"),
            "initial": "s0",
            "states": {"s0": {"bad": false}, "s1": {"bad": false}, "s2": {"bad": true}},
            "transitions": transitions,
            "terminal_states": ["s2"],
            "invariants": [{"name": "safe", "assert": [
                {"path": "/bad", "op": "eq", "right": {"value": false}}
            ]}]
        });
        let report = check_json(&raw.to_string(), DEFAULT_MAX_STATES).unwrap();
        let reachable: BTreeSet<usize> = (0..3).filter(|s| reach.contains(&(0, *s))).collect();
        assert_eq!(report.reachable_states, reachable.len(), "graph {mask}");
        let unsafe_states = reachable
            .iter()
            .filter(|s| **s == 2)
            .map(|s| format!("s{s}"))
            .collect();
        assert_eq!(targets(&report, "invariant-violation"), unsafe_states);
        let deadlocks = reachable
            .iter()
            .filter(|s| **s != 2 && !edges.iter().any(|(from, _)| from == *s))
            .map(|s| format!("s{s}"))
            .collect();
        assert_eq!(targets(&report, "nonterminal-deadlock"), deadlocks);
        let trapped = reachable
            .iter()
            .filter(|s| !reach.contains(&(**s, 2)))
            .map(|s| format!("s{s}"))
            .collect();
        assert_eq!(targets(&report, "terminal-unreachable"), trapped);
        for violation in &report.violations {
            assert_eq!(violation.trace.first().unwrap().state, "s0");
            for pair in violation.trace.windows(2) {
                assert!(transitions.iter().any(|edge| {
                    edge["from"] == pair[0].state
                        && edge["to"] == pair[1].state
                        && edge["event"].as_str() == pair[1].event.as_deref()
                }));
            }
        }
    }
}

#[test]
fn strict_profile_rejects_policy_bypasses_and_specification_drift() {
    let passing = comparison("0", "0", "eq");
    assert!(
        check_json_strict(&passing, DEFAULT_MAX_STATES)
            .unwrap()
            .passed()
    );

    let no_invariants = json!({
        "$schema": "des/state-machine/v1",
        "name": "no safety property",
        "initial": "done",
        "states": {"done": {}},
        "terminal_states": ["done"]
    });
    assert!(
        check_json_strict(&no_invariants.to_string(), DEFAULT_MAX_STATES)
            .unwrap_err()
            .contains("at least one safety invariant")
    );

    let disabled_checks = json!({
        "$schema": "des/state-machine/v1",
        "name": "disabled baseline",
        "initial": "done",
        "states": {"done": {"ok": true}},
        "terminal_states": ["done"],
        "checks": {
            "deterministic_events": false,
            "nonterminal_deadlocks": true,
            "terminal_reachability": true
        },
        "invariants": [{
            "name": "safe",
            "assert": [{"path": "/ok", "op": "eq", "right": {"value": true}}]
        }]
    });
    assert!(
        check_json_strict(&disabled_checks.to_string(), DEFAULT_MAX_STATES)
            .unwrap_err()
            .contains("deterministic_events")
    );

    let unreachable_state = json!({
        "$schema": "des/state-machine/v1",
        "name": "drift",
        "initial": "done",
        "states": {"done": {"ok": true}, "unused": {"ok": true}},
        "terminal_states": ["done"],
        "invariants": [{
            "name": "safe",
            "assert": [{"path": "/ok", "op": "eq", "right": {"value": true}}]
        }]
    });
    assert!(
        check_json_strict(&unreachable_state.to_string(), DEFAULT_MAX_STATES)
            .unwrap_err()
            .contains("warning")
    );

    let vacuous = json!({
        "$schema": "des/state-machine/v1",
        "name": "vacuous",
        "initial": "done",
        "states": {"done": {"kind": "ordinary", "amount": 0}},
        "terminal_states": ["done"],
        "invariants": [{
            "name": "protected amount",
            "when": [{"path": "/kind", "op": "eq", "right": {"value": "protected"}}],
            "assert": [{"path": "/amount", "op": "gte", "right": {"value": 0}}]
        }]
    });
    assert!(
        check_json_strict(&vacuous.to_string(), DEFAULT_MAX_STATES)
            .unwrap_err()
            .contains("guard never matched")
    );

    // Generic checking deliberately remains warning-tolerant for exploratory use.
    assert!(
        check_json(&vacuous.to_string(), DEFAULT_MAX_STATES)
            .unwrap()
            .passed()
    );
}

#[test]
fn strict_profile_rejects_duplicate_edges_and_nonabsorbing_terminals() {
    let duplicate = json!({
        "$schema": "des/state-machine/v1",
        "name": "duplicate edge",
        "initial": "start",
        "states": {"start": {"ok": true}, "done": {"ok": true}},
        "transitions": [
            {"event": "finish", "from": "start", "to": "done"},
            {"event": "finish", "from": "start", "to": "done"}
        ],
        "terminal_states": ["done"],
        "invariants": [{
            "name": "safe",
            "assert": [{"path": "/ok", "op": "eq", "right": {"value": true}}]
        }]
    });
    assert!(
        check_json(&duplicate.to_string(), DEFAULT_MAX_STATES)
            .unwrap()
            .passed()
    );
    assert!(
        check_json_strict(&duplicate.to_string(), DEFAULT_MAX_STATES)
            .unwrap_err()
            .contains("duplicate transition")
    );

    let terminal_loop = json!({
        "$schema": "des/state-machine/v1",
        "name": "terminal loop",
        "initial": "done",
        "states": {"done": {"ok": true}},
        "transitions": [{"event": "again", "from": "done", "to": "done"}],
        "terminal_states": ["done"],
        "invariants": [{
            "name": "safe",
            "assert": [{"path": "/ok", "op": "eq", "right": {"value": true}}]
        }]
    });
    assert!(
        check_json(&terminal_loop.to_string(), DEFAULT_MAX_STATES)
            .unwrap()
            .passed()
    );
    assert!(
        check_json_strict(&terminal_loop.to_string(), DEFAULT_MAX_STATES)
            .unwrap_err()
            .contains("absorbing")
    );
}

#[test]
fn markdown_evidence_cannot_be_broken_by_model_labels() {
    let raw = json!({
        "$schema": "des/state-machine/v1",
        "name": "<b>`proof`</b>",
        "initial": "start",
        "states": {"start": {"ok": true}, "```bad": {"ok": false}},
        "transitions": [{"event": "```go", "from": "start", "to": "```bad"}],
        "terminal_states": ["```bad"],
        "invariants": [{
            "name": "safe",
            "assert": [{"path": "/ok", "op": "eq", "right": {"value": true}}]
        }]
    });
    let report = check_json(&raw.to_string(), DEFAULT_MAX_STATES).unwrap();
    assert!(!report.passed());
    let markdown = report.render_markdown();
    assert!(markdown.contains("&lt;b&gt;\\`proof\\`&lt;/b&gt;"));
    assert!(!markdown.contains("```text"));
    assert!(markdown.contains("    start"));
    assert!(markdown.contains("    --```go--> ```bad"));
}

#[test]
fn pointer_size_and_warning_volume_are_bounded() {
    let too_long = format!("/{}", "a".repeat(4_096));
    let oversized_pointer = json!({
        "$schema": "des/state-machine/v1",
        "name": "oversized pointer",
        "initial": "done",
        "states": {"done": {"ok": true}},
        "terminal_states": ["done"],
        "invariants": [{
            "name": "oversized path",
            "assert": [{"path": too_long, "op": "exists"}]
        }]
    });
    assert!(
        check_json(&oversized_pointer.to_string(), DEFAULT_MAX_STATES)
            .unwrap_err()
            .contains("JSON Pointer exceeds")
    );

    let invariants: Vec<Value> = (0..150)
        .map(|index| {
            json!({
                "name": format!("vacuous-{index}"),
                "when": [{"path": "/kind", "op": "eq", "right": {"value": "protected"}}],
                "assert": [{"path": "/amount", "op": "gte", "right": {"value": 0}}]
            })
        })
        .collect();
    let many_warnings = json!({
        "$schema": "des/state-machine/v1",
        "name": "bounded warnings",
        "initial": "done",
        "states": {"done": {"kind": "ordinary", "amount": 0}},
        "terminal_states": ["done"],
        "invariants": invariants
    });
    let report = check_json(&many_warnings.to_string(), DEFAULT_MAX_STATES).unwrap();
    assert!(report.passed());
    assert_eq!(report.warning_count(), 150);
    assert_eq!(report.warnings.len(), 100);
    assert_eq!(report.omitted_warnings, 50);
    assert!(
        report
            .render_markdown()
            .contains("50 additional warning(s) omitted")
    );
    assert!(
        check_json_strict(&many_warnings.to_string(), DEFAULT_MAX_STATES)
            .unwrap_err()
            .contains("150 warning(s)")
    );
}

#[test]
fn retained_counterexample_traces_have_a_fixed_memory_bound() {
    let last = 2_100usize;
    let states: serde_json::Map<String, Value> = (0..=last)
        .map(|index| (format!("s{index:04}"), json!({"bad": index == last})))
        .collect();
    let transitions: Vec<Value> = (0..last)
        .map(|index| {
            json!({
                "event": "next",
                "from": format!("s{index:04}"),
                "to": format!("s{:04}", index + 1)
            })
        })
        .collect();
    let raw = json!({
        "$schema": "des/state-machine/v1",
        "name": "long counterexample",
        "initial": "s0000",
        "states": states,
        "transitions": transitions,
        "terminal_states": [format!("s{last:04}")],
        "invariants": [{
            "name": "never bad",
            "assert": [{"path": "/bad", "op": "eq", "right": {"value": false}}]
        }]
    });

    let report = check_json(&raw.to_string(), DEFAULT_MAX_STATES).unwrap();
    let violation = report
        .violations
        .iter()
        .find(|violation| violation.code == "invariant-violation")
        .unwrap();
    assert_eq!(violation.trace.len(), 2_048);
    assert_eq!(violation.trace_omitted_steps, 53);
    assert_eq!(violation.trace.first().unwrap().state, "s0000");
    assert_eq!(violation.trace.last().unwrap().state, "s2100");
    assert!(
        report
            .render_markdown()
            .contains("53 intermediate step(s) omitted")
    );
}
