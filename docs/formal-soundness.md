# Formal checker soundness boundaries

This follow-up hardens the verifier introduced by PR #28. It does not claim production conformance, arbitrary-precision arithmetic, or fairness-based liveness.

## Input is part of the proof obligation

The library and CLI reject JSON object keys that repeat at any depth, including escaped spellings of the same key. This check happens before maps can discard duplicate state identities or property values. Integer tokens outside the signed-i64/unsigned-u64 domain are rejected instead of silently becoming approximate floats. Model bytes are capped at 4,000,000; the CLI also limits reads, regular-file inputs, and the number of files. Excessive predicate work is rejected before evaluation.

## Numeric semantics

Integer/integer comparisons use an exact i128 intermediate. Mixed integer/f64 comparisons preserve the integer and compare against the floating operand's integral and fractional parts; they never round the integer to f64. Equality uses the same numeric ordering, recursively inside arrays and objects. Consequently `1` equals `1.0`, and `9007199254740993 <= 9007199254740992.0` is false.

Decimal/exponent literals retain serde_json's binary64 semantics, including decimal parsing/rounding. This is not exact-real arithmetic. Use bounded scaled integers for exact counters and amounts, or a separately specified arithmetic oracle. Reference: RFC 8259 section 6 and serde_json Number documentation.

## Missing inputs versus optional inputs

A missing comparison operand produces an invalid-invariant-predicate failure, including in a conditional guard. Missing data must not silently turn a safety requirement off. Use an explicit `exists`/`absent` predicate to model optional fields. Guards are evaluated in order and short-circuit, so put a presence guard before a comparison of an optional field.

A conditional invariant whose guard never matches any reachable state produces a vacuity warning. Warnings remain distinct from failures; callers must review them rather than mistake them for exercised coverage.

## Independent checks and negative controls

`tests/formal_soundness.rs` cross-checks all 512 directed graphs on three states against a relation-composition oracle independent of the production BFS/reverse-BFS implementation. It compares safety failures, deadlocks, terminal reachability, reachable-state counts, and returned trace edges. Separate regression tests cover precision boundaries, duplicate identities, missing guards, integer overflow, and input limits.

The regression-first CI run on commit `9c9d3874763b795416f07c0e6a1016b4bfbcac08` compiled and ran eight tests: six failed as expected on the parent checker, while the graph oracle and malformed-literal-operand test passed. The latter is a preserved guard, not a newly discovered bug. Final exact-head results belong in the PR evidence.

## Review and rollout

This is a stacked delta targeting `feat/bounded-formal-model-checker`; merge the correction into #28 before accepting that feature. The dedicated workflow only covers that stacked base; the existing main-target CI covers #28. No checks are disabled and no production services are changed. `default-run` preserves the existing MCP `cargo run` entry point after adding a second binary.
