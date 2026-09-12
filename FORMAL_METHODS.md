# Formal methods for DES state machines

`des-formal-check` is a bounded, explicit-state model checker for the small state machines whose failures have disproportionate operational impact: leases, schedulers, workflow lifecycles, authorization flows, job orchestration, and simulation controllers.

It is intentionally narrower than a general theorem prover. The checker proves obligations over every state and transition in a finite JSON abstraction; it does **not** claim that arbitrary implementation code is correct merely because its model passes.

## What the checker establishes

For every state reachable from `initial`, the checker can enforce:

- safety invariants expressed as JSON Pointer predicates;
- deterministic event handling: one `(state, event)` pair cannot select multiple target states;
- absence of reachable non-terminal deadlocks;
- terminal reachability: every reachable state retains at least one path to a declared terminal state.

Failures include a shortest counterexample trace from the initial state. Unreachable states are reported as warnings, because they usually indicate specification drift but do not falsify a safety claim about reachable behavior.

The result is exhaustive for the supplied explicit graph, up to the configured state bound. Terminal reachability is an existential property—each reachable state has *some* path to a terminal state—not a fairness proof that every execution eventually terminates.

## Model format

```json
{
  "$schema": "des/state-machine/v1",
  "name": "routing solve lifecycle",
  "initial": "queued",
  "states": {
    "queued": { "status": "queued", "done": 0, "total": 2 },
    "running": { "status": "running", "done": 1, "total": 2 },
    "completed": { "status": "completed", "done": 2, "total": 2 }
  },
  "transitions": [
    { "event": "start", "from": "queued", "to": "running" },
    { "event": "finish", "from": "running", "to": "completed" }
  ],
  "invariants": [
    {
      "name": "progress never exceeds total work",
      "assert": [
        {
          "path": "/done",
          "op": "lte",
          "right": { "path": "/total" }
        }
      ]
    },
    {
      "name": "completed means all work is done",
      "when": [
        {
          "path": "/status",
          "op": "eq",
          "right": { "value": "completed" }
        }
      ],
      "assert": [
        {
          "path": "/done",
          "op": "eq",
          "right": { "path": "/total" }
        }
      ]
    }
  ],
  "terminal_states": ["completed"],
  "checks": {
    "deterministic_events": true,
    "nonterminal_deadlocks": true,
    "terminal_reachability": true
  }
}
```

Each named state contains a JSON object. Predicates address that object with RFC 6901 JSON Pointers. An empty pointer addresses the complete state object.

Supported operators:

| Operator | Meaning |
| --- | --- |
| `eq`, `ne` | JSON equality or inequality |
| `lt`, `lte`, `gt`, `gte` | ordered comparison between two numbers or two strings |
| `exists`, `absent` | field-presence test; these do not take `right` |

A comparison operand is explicit, so a literal JSON `null` remains distinguishable from a missing operand:

```json
{ "path": "/owner", "op": "eq", "right": { "value": null } }
{ "path": "/done", "op": "lte", "right": { "path": "/total" } }
```

`when` predicates are conjoined and make an invariant conditional. `assert` predicates are also conjoined; each failed assertion is reported independently.

## Running it

```sh
cargo run --locked --bin des-formal-check -- formal/**/*.json
cargo run --locked --bin des-formal-check -- --max-states 25000 formal/lease.json
```

Exit codes are stable for CI:

- `0`: every model passed;
- `1`: at least one well-formed model produced a counterexample;
- `2`: usage, file, schema, or other model-input error.

The default bound is 10,000 declared states and the hard ceiling is 100,000. Raising the bound should be a reviewed decision, not an automatic response to state-space growth.

## Connecting the model to implementation

A model check is useful only when the abstraction remains connected to the code it represents. Each adopting repository should add conformance tests that:

1. enumerate the implementation's public states and events;
2. verify every implementation transition is permitted by the model;
3. replay model counterexample traces against the implementation where practical;
4. pin model evidence to the exact implementation commit tested;
5. document abstractions, omitted data, fairness assumptions, and environmental assumptions.

For concurrent implementations, pair this explicit-state layer with schedule exploration such as Loom, deterministic simulation, or a protocol-specific TLA+/PlusCal model. For arithmetic-heavy algorithms, pair it with property tests or proof-oriented tools. These techniques answer different questions and reinforce one another.

## Review checklist

A formal-model PR should identify:

- the production failure class being excluded;
- the safety and progress properties encoded;
- the state bound and reachable-state count;
- the shortest counterexamples observed before the fix;
- implementation conformance tests;
- assumptions that remain outside the model.

Never turn a failing obligation off merely to make CI green. Narrow an invalid property only when the revised assumption is explicit, reviewed, and backed by implementation evidence.
