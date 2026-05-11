# Pipeline: state-flow-neighborhoods

Wave: `semantic-proof-gate`

## Task 13 - Data-flow and state-flow neighborhoods

Add lightweight data-flow edges after behavior facts and callsites have stable ids.

## Scope

Start with TypeScript/JavaScript and Rust.

Cover:
- Assignments.
- Returns.
- Parameter-to-call propagation.
- Exported values.
- Config/env value consumption.

## Required Behaviors

- Data-flow edges connect variables, parameters, and config facts to consuming symbols.
- Returned values link producer to caller when statically obvious.
- Env/config reads connect fact ids to functions.
- Low-confidence edges are visible rather than hidden.

## Boundaries

- No full SSA.
- No runtime evaluation.

## Verification

- Add focused parser/indexer/store/search tests.
- Verify the data-flow edges appear in relevant neighborhoods and evidence output without overwhelming defaults.

