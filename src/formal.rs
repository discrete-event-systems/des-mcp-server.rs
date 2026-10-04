//! Bounded explicit-state checking for small, critical DES state machines.
//!
//! The checker exhaustively traverses the finite graph supplied by the model.
//! It proves properties of that abstraction, not arbitrary implementation code.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Deserialize;
use serde_json::Value;

mod input;
mod numeric;

pub const MODEL_SCHEMA: &str = "des/state-machine/v1";
pub const DEFAULT_MAX_STATES: usize = 10_000;
pub const HARD_MAX_STATES: usize = 100_000;
pub const MAX_MODEL_BYTES: usize = 4_000_000;
const MAX_PREDICATE_VISITS: usize = 2_000_000;
const MAX_POINTER_WORK_BYTES: usize = 64_000_000;
const MAX_JSON_POINTER_BYTES: usize = 4_096;
const MAX_VIOLATIONS: usize = 100;
const MAX_WARNINGS: usize = 100;
const MAX_TRACE_STEPS: usize = 2_048;
const TRACE_EDGE_STEPS: usize = MAX_TRACE_STEPS / 2;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateMachine {
    #[serde(rename = "$schema")]
    schema: String,
    name: String,
    initial: String,
    states: BTreeMap<String, Value>,
    #[serde(default)]
    transitions: Vec<Transition>,
    #[serde(default)]
    invariants: Vec<Invariant>,
    #[serde(default)]
    terminal_states: Vec<String>,
    #[serde(default)]
    checks: CheckOptions,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transition {
    event: String,
    from: String,
    to: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Invariant {
    name: String,
    #[serde(default)]
    when: Vec<Predicate>,
    #[serde(default, rename = "assert")]
    assertions: Vec<Predicate>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Predicate {
    path: String,
    op: Operator,
    #[serde(default)]
    right: Option<Operand>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum Operand {
    Path(PathOperand),
    Value(ValueOperand),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathOperand {
    path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ValueOperand {
    value: Value,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Operator {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    Exists,
    Absent,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckOptions {
    #[serde(default = "yes")]
    deterministic_events: bool,
    #[serde(default = "yes")]
    nonterminal_deadlocks: bool,
    #[serde(default = "yes")]
    terminal_reachability: bool,
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self {
            deterministic_events: true,
            nonterminal_deadlocks: true,
            terminal_reachability: true,
        }
    }
}

const fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceStep {
    pub state: String,
    pub event: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub code: &'static str,
    pub message: String,
    pub trace: Vec<TraceStep>,
    pub trace_omitted_steps: usize,
    pub trace_omission_after: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    pub model_name: String,
    pub declared_states: usize,
    pub reachable_states: usize,
    pub transitions: usize,
    pub invariants: usize,
    pub violations: Vec<Violation>,
    pub omitted_violations: usize,
    pub warnings: Vec<String>,
    pub omitted_warnings: usize,
}

impl CheckReport {
    #[must_use]
    pub fn passed(&self) -> bool {
        self.violations.is_empty() && self.omitted_violations == 0
    }

    #[must_use]
    pub fn violation_count(&self) -> usize {
        self.violations.len() + self.omitted_violations
    }

    #[must_use]
    pub fn warning_count(&self) -> usize {
        self.warnings.len() + self.omitted_warnings
    }

    #[must_use]
    pub fn render_markdown(&self) -> String {
        let status = if self.passed() { "PASS" } else { "FAIL" };
        let mut out = format!(
            "# Formal state-machine check: {} — {status}\n\n\
             - reachable states: {} / {}\n\
             - transitions: {}\n\
             - invariants: {}\n\
             - violations: {}\n\
             - warnings: {}\n",
            markdown_text(&self.model_name),
            self.reachable_states,
            self.declared_states,
            self.transitions,
            self.invariants,
            self.violation_count(),
            self.warning_count()
        );
        if !self.violations.is_empty() {
            out.push_str("\n## Violations\n");
            for violation in &self.violations {
                out.push_str(&format!(
                    "\n### `{}`\n\n{}\n",
                    violation.code,
                    markdown_text(&violation.message)
                ));
                if !violation.trace.is_empty() {
                    out.push_str("\nShortest counterexample trace:\n\n");
                    for line in render_trace(violation).lines() {
                        out.push_str("    ");
                        out.push_str(line);
                        out.push('\n');
                    }
                }
            }
            if self.omitted_violations > 0 {
                out.push_str(&format!(
                    "\n_{} additional violation(s) omitted._\n",
                    self.omitted_violations
                ));
            }
        }
        if self.warning_count() > 0 {
            out.push_str("\n## Warnings\n");
            for warning in &self.warnings {
                out.push_str(&format!("\n- {}", markdown_text(warning)));
            }
            if self.omitted_warnings > 0 {
                out.push_str(&format!(
                    "\n\n_{} additional warning(s) omitted._",
                    self.omitted_warnings
                ));
            }
            out.push('\n');
        }
        out
    }
}

/// Parse and exhaustively check a finite explicit-state model.
/// Ambiguous JSON, overflowing integer tokens, and oversized inputs fail closed.
pub fn check_json(raw: &str, max_states: usize) -> Result<CheckReport, String> {
    let model = parse_model(raw)?;
    check_model(&model, max_states)
}

/// Apply the organization baseline on top of ordinary model checking.
///
/// Strict models must keep all baseline graph obligations enabled, contain at
/// least one safety invariant, and produce no specification-drift warnings.
/// This is intended for CI policy gates; callers that intentionally need a
/// weaker or exploratory model can continue to use `check_json`.
pub fn check_json_strict(raw: &str, max_states: usize) -> Result<CheckReport, String> {
    let model = parse_model(raw)?;
    let report = check_model(&model, max_states)?;
    validate_strict_profile(&model)?;
    // Preserve real counterexamples as exit-1 proof failures. Warning-only
    // specifications are rejected as invalid strict-policy evidence.
    if report.passed() && report.warning_count() > 0 {
        let preview = report
            .warnings
            .iter()
            .take(5)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "strict verification rejects {} warning(s): {preview}",
            report.warning_count()
        ));
    }
    Ok(report)
}

fn parse_model(raw: &str) -> Result<StateMachine, String> {
    input::validate_json(raw)?;
    serde_json::from_str(raw).map_err(|error| format!("invalid model JSON: {error}"))
}

fn validate_strict_profile(model: &StateMachine) -> Result<(), String> {
    if model.invariants.is_empty() {
        return Err("strict verification requires at least one safety invariant".to_string());
    }
    let mut disabled = Vec::new();
    if !model.checks.deterministic_events {
        disabled.push("deterministic_events");
    }
    if !model.checks.nonterminal_deadlocks {
        disabled.push("nonterminal_deadlocks");
    }
    if !model.checks.terminal_reachability {
        disabled.push("terminal_reachability");
    }
    if !disabled.is_empty() {
        return Err(format!(
            "strict verification requires baseline checks to remain enabled: {}",
            disabled.join(", ")
        ));
    }

    let terminals: BTreeSet<&str> = model.terminal_states.iter().map(String::as_str).collect();
    let mut transitions = BTreeSet::new();
    for transition in &model.transitions {
        let identity = (
            transition.from.as_str(),
            transition.event.as_str(),
            transition.to.as_str(),
        );
        if !transitions.insert(identity) {
            return Err(format!(
                "strict verification rejects duplicate transition {:?}: {:?} -> {:?}",
                transition.event, transition.from, transition.to
            ));
        }
        if terminals.contains(transition.from.as_str()) {
            return Err(format!(
                "strict verification requires terminal state {:?} to be absorbing",
                transition.from
            ));
        }
    }
    Ok(())
}

fn check_model(model: &StateMachine, max_states: usize) -> Result<CheckReport, String> {
    validate(model, max_states)?;
    let outgoing = outgoing(model);
    let incoming = incoming(model);
    let (reachable, order, predecessors) = explore(model, &outgoing);
    let terminals: BTreeSet<String> = model.terminal_states.iter().cloned().collect();
    let mut report = CheckReport {
        model_name: model.name.clone(),
        declared_states: model.states.len(),
        reachable_states: reachable.len(),
        transitions: model.transitions.len(),
        invariants: model.invariants.len(),
        violations: Vec::new(),
        omitted_violations: 0,
        warnings: Vec::new(),
        omitted_warnings: 0,
    };

    check_invariants(model, &order, &predecessors, &mut report);
    if model.checks.deterministic_events {
        check_determinism(model, &reachable, &predecessors, &mut report);
    }
    if model.checks.nonterminal_deadlocks {
        check_deadlocks(
            model,
            &reachable,
            &terminals,
            &outgoing,
            &predecessors,
            &mut report,
        );
    }
    if model.checks.terminal_reachability {
        check_terminal_reachability(
            model,
            &reachable,
            &terminals,
            &incoming,
            &predecessors,
            &mut report,
        );
    }

    let unreachable: Vec<&str> = model
        .states
        .keys()
        .filter(|state| !reachable.contains(*state))
        .map(String::as_str)
        .collect();
    if !unreachable.is_empty() {
        add_warning(
            &mut report,
            format!(
                "{} unreachable state(s): {}",
                unreachable.len(),
                quoted(unreachable.iter().copied().take(50))
            ),
        );
    }
    Ok(report)
}

fn validate(model: &StateMachine, max_states: usize) -> Result<(), String> {
    if !(1..=HARD_MAX_STATES).contains(&max_states) {
        return Err(format!(
            "max_states must be between 1 and {HARD_MAX_STATES}, got {max_states}"
        ));
    }
    if model.schema != MODEL_SCHEMA {
        return Err(format!(
            "unsupported $schema {:?}; expected {MODEL_SCHEMA:?}",
            model.schema
        ));
    }
    label(&model.name, "model name")?;
    if model.states.is_empty() {
        return Err("states must not be empty".to_string());
    }
    if model.states.len() > max_states {
        return Err(format!(
            "model declares {} states, exceeding max_states={max_states}",
            model.states.len()
        ));
    }
    let predicates = model.invariants.iter().fold(0usize, |sum, invariant| {
        sum.saturating_add(invariant.when.len())
            .saturating_add(invariant.assertions.len())
    });
    if model.states.len().saturating_mul(predicates) > MAX_PREDICATE_VISITS {
        return Err(format!(
            "predicate work exceeds {MAX_PREDICATE_VISITS} state/predicate visits"
        ));
    }
    let pointer_bytes = model.invariants.iter().fold(0usize, |sum, invariant| {
        invariant
            .when
            .iter()
            .chain(&invariant.assertions)
            .fold(sum, |sum, predicate| {
                let right_path_bytes = match predicate.right.as_ref() {
                    Some(Operand::Path(path)) => path.path.len(),
                    Some(Operand::Value(_)) | None => 0,
                };
                sum.saturating_add(predicate.path.len())
                    .saturating_add(right_path_bytes)
            })
    });
    if model.states.len().saturating_mul(pointer_bytes) > MAX_POINTER_WORK_BYTES {
        return Err(format!(
            "JSON Pointer work exceeds {MAX_POINTER_WORK_BYTES} state/pointer bytes"
        ));
    }
    if !model.states.contains_key(&model.initial) {
        return Err(format!("unknown initial state {:?}", model.initial));
    }
    for (name, state) in &model.states {
        label(name, "state name")?;
        if !state.is_object() {
            return Err(format!("state {name:?} must be a JSON object"));
        }
    }
    let mut terminals = BTreeSet::new();
    for terminal in &model.terminal_states {
        if !model.states.contains_key(terminal) {
            return Err(format!("unknown terminal state {terminal:?}"));
        }
        if !terminals.insert(terminal) {
            return Err(format!("duplicate terminal state {terminal:?}"));
        }
    }
    if model.checks.terminal_reachability && terminals.is_empty() {
        return Err("terminal_reachability requires a terminal state".to_string());
    }
    for transition in &model.transitions {
        label(&transition.event, "event name")?;
        if !model.states.contains_key(&transition.from)
            || !model.states.contains_key(&transition.to)
        {
            return Err(format!(
                "transition {:?}: {:?} -> {:?} references an unknown state",
                transition.event, transition.from, transition.to
            ));
        }
    }
    let mut invariant_names = BTreeSet::new();
    for invariant in &model.invariants {
        label(&invariant.name, "invariant name")?;
        if !invariant_names.insert(invariant.name.as_str()) {
            return Err(format!("duplicate invariant name {:?}", invariant.name));
        }
        if invariant.assertions.is_empty() {
            return Err(format!("invariant {:?} has no assertions", invariant.name));
        }
        for predicate in invariant.when.iter().chain(&invariant.assertions) {
            validate_predicate(model, predicate, &invariant.name)?;
        }
    }
    Ok(())
}

fn label(value: &str, what: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || value.chars().any(unsafe_display_character)
    {
        return Err(format!("invalid {what}: {value:?}"));
    }
    Ok(())
}

fn unsafe_display_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

fn validate_predicate(
    model: &StateMachine,
    predicate: &Predicate,
    invariant: &str,
) -> Result<(), String> {
    pointer(&predicate.path)?;
    if predicate.op != Operator::Absent && !exists_anywhere(model, &predicate.path) {
        return Err(format!(
            "invariant {invariant:?} path {:?} is absent from every state",
            predicate.path
        ));
    }
    match predicate.op {
        Operator::Exists | Operator::Absent if predicate.right.is_some() => Err(format!(
            "invariant {invariant:?}: {:?} must not have right",
            predicate.op
        )),
        Operator::Exists | Operator::Absent => Ok(()),
        _ => match predicate.right.as_ref() {
            None => Err(format!(
                "invariant {invariant:?}: {:?} requires right",
                predicate.op
            )),
            Some(Operand::Path(path)) => {
                pointer(&path.path)?;
                if exists_anywhere(model, &path.path) {
                    Ok(())
                } else {
                    Err(format!(
                        "invariant {invariant:?} right path {:?} is absent from every state",
                        path.path
                    ))
                }
            }
            Some(Operand::Value(_)) => Ok(()),
        },
    }
}

fn pointer(path: &str) -> Result<(), String> {
    if path.len() > MAX_JSON_POINTER_BYTES {
        return Err(format!(
            "JSON Pointer exceeds {MAX_JSON_POINTER_BYTES} bytes"
        ));
    }
    if path.is_empty() {
        return Ok(());
    }
    if !path.starts_with('/') {
        return Err(format!("invalid JSON Pointer {path:?}"));
    }
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'~' {
            if index + 1 >= bytes.len() || !matches!(bytes[index + 1], b'0' | b'1') {
                return Err(format!("invalid JSON Pointer escape in {path:?}"));
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    Ok(())
}

fn exists_anywhere(model: &StateMachine, path: &str) -> bool {
    model.states.values().any(|state| at(state, path).is_some())
}

type EdgeMap<'a> = BTreeMap<&'a str, Vec<&'a Transition>>;
type Predecessors = BTreeMap<String, (String, String)>;

fn outgoing(model: &StateMachine) -> EdgeMap<'_> {
    let mut edges: EdgeMap<'_> = BTreeMap::new();
    for transition in &model.transitions {
        edges
            .entry(transition.from.as_str())
            .or_default()
            .push(transition);
    }
    for list in edges.values_mut() {
        list.sort_by(|left, right| {
            left.event
                .cmp(&right.event)
                .then_with(|| left.to.cmp(&right.to))
        });
    }
    edges
}

fn incoming(model: &StateMachine) -> BTreeMap<&str, Vec<&str>> {
    let mut edges: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for transition in &model.transitions {
        edges
            .entry(transition.to.as_str())
            .or_default()
            .push(transition.from.as_str());
    }
    edges
}

fn explore(
    model: &StateMachine,
    edges: &EdgeMap<'_>,
) -> (BTreeSet<String>, Vec<String>, Predecessors) {
    let mut seen = BTreeSet::new();
    let mut order = Vec::new();
    let mut predecessors = BTreeMap::new();
    let mut queue = VecDeque::from([model.initial.clone()]);
    seen.insert(model.initial.clone());
    while let Some(state) = queue.pop_front() {
        order.push(state.clone());
        for transition in edges.get(state.as_str()).into_iter().flatten() {
            if seen.insert(transition.to.clone()) {
                predecessors.insert(
                    transition.to.clone(),
                    (state.clone(), transition.event.clone()),
                );
                queue.push_back(transition.to.clone());
            }
        }
    }
    (seen, order, predecessors)
}

fn check_invariants(
    model: &StateMachine,
    order: &[String],
    predecessors: &Predecessors,
    report: &mut CheckReport,
) {
    let mut guard_hits = vec![0usize; model.invariants.len()];
    for state_name in order {
        let state = &model.states[state_name];
        for (index, invariant) in model.invariants.iter().enumerate() {
            let mut applies = true;
            for predicate in &invariant.when {
                match evaluate(predicate, state) {
                    Ok(true) => {}
                    Ok(false) => {
                        applies = false;
                        break;
                    }
                    Err(error) => {
                        add(
                            report,
                            "invalid-invariant-predicate",
                            format!(
                                "invariant {:?} guard failed in {state_name:?}: {error}",
                                invariant.name
                            ),
                            || trace(&model.initial, state_name, predecessors),
                        );
                        applies = false;
                        break;
                    }
                }
            }
            if !applies {
                continue;
            }
            guard_hits[index] += 1;
            for predicate in &invariant.assertions {
                match evaluate(predicate, state) {
                    Ok(true) => {}
                    Ok(false) => add(
                        report,
                        "invariant-violation",
                        format!(
                            "invariant {:?} failed in {state_name:?}: {}",
                            invariant.name,
                            describe(predicate)
                        ),
                        || trace(&model.initial, state_name, predecessors),
                    ),
                    Err(error) => add(
                        report,
                        "invalid-invariant-predicate",
                        format!(
                            "invariant {:?} assertion failed in {state_name:?}: {error}",
                            invariant.name
                        ),
                        || trace(&model.initial, state_name, predecessors),
                    ),
                }
            }
        }
    }
    for (invariant, hits) in model.invariants.iter().zip(guard_hits) {
        if !invariant.when.is_empty() && hits == 0 {
            add_warning(
                report,
                format!(
                    "invariant {:?} guard never matched a reachable state",
                    invariant.name
                ),
            );
        }
    }
}

fn check_determinism(
    model: &StateMachine,
    reachable: &BTreeSet<String>,
    predecessors: &Predecessors,
    report: &mut CheckReport,
) {
    let mut targets: BTreeMap<(&str, &str), BTreeSet<&str>> = BTreeMap::new();
    for transition in &model.transitions {
        if reachable.contains(&transition.from) {
            targets
                .entry((transition.from.as_str(), transition.event.as_str()))
                .or_default()
                .insert(transition.to.as_str());
        }
    }
    for ((state, event), destinations) in targets {
        if destinations.len() > 1 {
            add(
                report,
                "nondeterministic-event",
                format!(
                    "state {state:?} maps event {event:?} to: {}",
                    quoted(destinations)
                ),
                || trace(&model.initial, state, predecessors),
            );
        }
    }
}

fn check_deadlocks(
    model: &StateMachine,
    reachable: &BTreeSet<String>,
    terminals: &BTreeSet<String>,
    edges: &EdgeMap<'_>,
    predecessors: &Predecessors,
    report: &mut CheckReport,
) {
    for state in reachable {
        if !terminals.contains(state) && !edges.contains_key(state.as_str()) {
            add(
                report,
                "nonterminal-deadlock",
                format!("non-terminal state {state:?} has no outgoing transition"),
                || trace(&model.initial, state, predecessors),
            );
        }
    }
}

fn check_terminal_reachability(
    model: &StateMachine,
    reachable: &BTreeSet<String>,
    terminals: &BTreeSet<String>,
    incoming: &BTreeMap<&str, Vec<&str>>,
    predecessors: &Predecessors,
    report: &mut CheckReport,
) {
    let mut can_finish = BTreeSet::new();
    let mut queue = VecDeque::new();
    for terminal in terminals {
        if reachable.contains(terminal) {
            can_finish.insert(terminal.clone());
            queue.push_back(terminal.clone());
        }
    }
    while let Some(state) = queue.pop_front() {
        for parent in incoming.get(state.as_str()).into_iter().flatten() {
            if reachable.contains(*parent) && can_finish.insert((*parent).to_string()) {
                queue.push_back((*parent).to_string());
            }
        }
    }
    for state in reachable {
        if !can_finish.contains(state) {
            add(
                report,
                "terminal-unreachable",
                format!("state {state:?} cannot reach a terminal state"),
                || trace(&model.initial, state, predecessors),
            );
        }
    }
}

fn evaluate(predicate: &Predicate, state: &Value) -> Result<bool, String> {
    let left = at(state, &predicate.path);
    match predicate.op {
        Operator::Exists => Ok(left.is_some()),
        Operator::Absent => Ok(left.is_none()),
        Operator::Eq | Operator::Ne => {
            let left =
                left.ok_or_else(|| format!("missing comparison path {:?}", predicate.path))?;
            let right = operand(predicate.right.as_ref(), state)
                .ok_or_else(|| "missing right comparison operand".to_string())?;
            let equal = numeric::equal(left, right);
            Ok(if predicate.op == Operator::Eq {
                equal
            } else {
                !equal
            })
        }
        Operator::Lt | Operator::Lte | Operator::Gt | Operator::Gte => {
            let left =
                left.ok_or_else(|| format!("missing comparison path {:?}", predicate.path))?;
            let right = operand(predicate.right.as_ref(), state)
                .ok_or_else(|| "missing right comparison operand".to_string())?;
            let ordering = ordered(left, right).ok_or_else(|| {
                "ordered comparison requires two numbers or two strings".to_string()
            })?;
            Ok(match predicate.op {
                Operator::Lt => ordering == Ordering::Less,
                Operator::Lte => ordering != Ordering::Greater,
                Operator::Gt => ordering == Ordering::Greater,
                Operator::Gte => ordering != Ordering::Less,
                Operator::Eq | Operator::Ne | Operator::Exists | Operator::Absent => {
                    unreachable!()
                }
            })
        }
    }
}

fn operand<'a>(right: Option<&'a Operand>, state: &'a Value) -> Option<&'a Value> {
    match right? {
        Operand::Path(path) => at(state, &path.path),
        Operand::Value(value) => Some(&value.value),
    }
}

fn at<'a>(state: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        Some(state)
    } else {
        state.pointer(path)
    }
}

fn ordered(left: &Value, right: &Value) -> Option<Ordering> {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => numeric::compare(left, right),
        (Value::String(left), Value::String(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

fn describe(predicate: &Predicate) -> String {
    let op = match predicate.op {
        Operator::Eq => "==",
        Operator::Ne => "!=",
        Operator::Lt => "<",
        Operator::Lte => "<=",
        Operator::Gt => ">",
        Operator::Gte => ">=",
        Operator::Exists => "exists",
        Operator::Absent => "is absent",
    };
    match &predicate.right {
        Some(Operand::Path(path)) => format!("{} {op} {}", predicate.path, path.path),
        Some(Operand::Value(value)) => format!("{} {op} {}", predicate.path, value.value),
        None => format!("{} {op}", predicate.path),
    }
}

struct BoundedTrace {
    steps: Vec<TraceStep>,
    omitted_steps: usize,
}

fn trace(initial: &str, target: &str, predecessors: &Predecessors) -> BoundedTrace {
    let mut full_reverse = Vec::with_capacity(MAX_TRACE_STEPS);
    let mut target_reverse = Vec::with_capacity(TRACE_EDGE_STEPS);
    let mut initial_reverse = VecDeque::with_capacity(TRACE_EDGE_STEPS + 1);
    let mut current = target.to_string();
    let mut total_steps = 0usize;

    loop {
        let event = if current == initial {
            None
        } else {
            predecessors.get(&current).map(|(_, event)| event.clone())
        };
        let step = TraceStep {
            state: current.clone(),
            event,
        };

        if full_reverse.len() < MAX_TRACE_STEPS {
            full_reverse.push(step.clone());
        }
        if total_steps < TRACE_EDGE_STEPS {
            target_reverse.push(step.clone());
        }
        initial_reverse.push_back(step);
        if initial_reverse.len() > TRACE_EDGE_STEPS {
            initial_reverse.pop_front();
        }
        total_steps += 1;

        if current == initial {
            break;
        }
        let Some((parent, _)) = predecessors.get(&current) else {
            break;
        };
        current = parent.clone();
    }

    if total_steps <= MAX_TRACE_STEPS {
        full_reverse.reverse();
        return BoundedTrace {
            steps: full_reverse,
            omitted_steps: 0,
        };
    }

    let mut steps: Vec<TraceStep> = initial_reverse.into_iter().rev().collect();
    target_reverse.reverse();
    steps.extend(target_reverse);
    BoundedTrace {
        omitted_steps: total_steps.saturating_sub(steps.len()),
        steps,
    }
}

fn markdown_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '!' | '|' => {
                out.push('\\');
                out.push(character);
            }
            _ => out.push(character),
        }
    }
    out
}

fn render_trace(violation: &Violation) -> String {
    let Some(first) = violation.trace.first() else {
        return String::new();
    };
    let mut out = format!("{}\n", first.state);
    let split = violation
        .trace_omission_after
        .unwrap_or(violation.trace.len())
        .min(violation.trace.len());

    for step in violation.trace[1..split].iter() {
        out.push_str(&format!(
            "  --{}--> {}\n",
            step.event.as_deref().unwrap_or("?"),
            step.state
        ));
    }

    if violation.trace_omitted_steps > 0 {
        out.push_str(&format!(
            "... {} intermediate step(s) omitted ...\n",
            violation.trace_omitted_steps
        ));
        if let Some(first_suffix) = violation.trace.get(split) {
            out.push_str(&format!("{}\n", first_suffix.state));
            for step in violation.trace[(split + 1)..].iter() {
                out.push_str(&format!(
                    "  --{}--> {}\n",
                    step.event.as_deref().unwrap_or("?"),
                    step.state
                ));
            }
        }
    }
    out
}

fn add_warning(report: &mut CheckReport, warning: String) {
    if report.warnings.len() < MAX_WARNINGS {
        report.warnings.push(warning);
    } else {
        report.omitted_warnings += 1;
    }
}

fn add<F>(report: &mut CheckReport, code: &'static str, message: String, trace: F)
where
    F: FnOnce() -> BoundedTrace,
{
    if report.violations.len() < MAX_VIOLATIONS {
        let trace = trace();
        report.violations.push(Violation {
            code,
            message,
            trace_omission_after: (trace.omitted_steps > 0).then_some(TRACE_EDGE_STEPS),
            trace: trace.steps,
            trace_omitted_steps: trace.omitted_steps,
        });
    } else {
        report.omitted_violations += 1;
    }
}

fn quoted<'a>(values: impl IntoIterator<Item = &'a str>) -> String {
    values
        .into_iter()
        .map(|value| format!("{value:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSING: &str = r#"{
      "$schema": "des/state-machine/v1",
      "name": "routing lifecycle",
      "initial": "queued",
      "states": {
        "queued": {"status": "queued", "done": 0, "total": 2},
        "running": {"status": "running", "done": 1, "total": 2},
        "completed": {"status": "completed", "done": 2, "total": 2}
      },
      "transitions": [
        {"event": "start", "from": "queued", "to": "running"},
        {"event": "finish", "from": "running", "to": "completed"}
      ],
      "invariants": [{
        "name": "bounded progress",
        "assert": [{"path": "/done", "op": "lte", "right": {"path": "/total"}}]
      }],
      "terminal_states": ["completed"]
    }"#;

    #[test]
    fn checks_every_reachable_state() {
        let report = check_json(PASSING, DEFAULT_MAX_STATES).unwrap();
        assert!(report.passed(), "{}", report.render_markdown());
        assert_eq!(report.reachable_states, 3);
    }

    #[test]
    fn returns_a_shortest_counterexample() {
        let failing = PASSING.replace(
            "\"completed\": {\"status\": \"completed\", \"done\": 2, \"total\": 2}",
            "\"completed\": {\"status\": \"completed\", \"done\": 3, \"total\": 2}",
        );
        let report = check_json(&failing, DEFAULT_MAX_STATES).unwrap();
        let violation = report
            .violations
            .iter()
            .find(|violation| violation.code == "invariant-violation")
            .unwrap();
        assert_eq!(
            violation
                .trace
                .iter()
                .map(|step| step.state.as_str())
                .collect::<Vec<_>>(),
            vec!["queued", "running", "completed"]
        );
    }

    #[test]
    fn detects_nondeterminism_deadlock_and_liveness_failure() {
        let raw = r#"{
          "$schema": "des/state-machine/v1",
          "name": "ambiguous lease",
          "initial": "free",
          "states": {
            "free": {"owner": null},
            "held-a": {"owner": "a"},
            "held-b": {"owner": "b"},
            "released": {"owner": null}
          },
          "transitions": [
            {"event": "acquire", "from": "free", "to": "held-a"},
            {"event": "acquire", "from": "free", "to": "held-b"},
            {"event": "release", "from": "held-a", "to": "released"}
          ],
          "terminal_states": ["released"]
        }"#;
        let report = check_json(raw, DEFAULT_MAX_STATES).unwrap();
        for code in [
            "nondeterministic-event",
            "nonterminal-deadlock",
            "terminal-unreachable",
        ] {
            assert!(
                report
                    .violations
                    .iter()
                    .any(|violation| violation.code == code)
            );
        }
    }

    #[test]
    fn enforces_schema_and_state_bound() {
        assert!(check_json(&PASSING.replace(MODEL_SCHEMA, "v2"), 10).is_err());
        assert!(check_json(PASSING, 2).is_err());
    }

    #[test]
    fn omitted_violations_do_not_construct_counterexample_traces() {
        use std::cell::Cell;

        let trace_builds = Cell::new(0usize);
        let mut report = CheckReport {
            model_name: "trace laziness".to_string(),
            declared_states: 1,
            reachable_states: 1,
            transitions: 0,
            invariants: 1,
            violations: Vec::new(),
            omitted_violations: 0,
            warnings: Vec::new(),
            omitted_warnings: 0,
        };

        for _ in 0..(MAX_VIOLATIONS + 25) {
            add(
                &mut report,
                "test-violation",
                "synthetic".to_string(),
                || {
                    trace_builds.set(trace_builds.get() + 1);
                    BoundedTrace {
                        steps: vec![TraceStep {
                            state: "s0".to_string(),
                            event: None,
                        }],
                        omitted_steps: 0,
                    }
                },
            );
        }

        assert_eq!(report.violations.len(), MAX_VIOLATIONS);
        assert_eq!(report.omitted_violations, 25);
        assert_eq!(trace_builds.get(), MAX_VIOLATIONS);
    }
}
