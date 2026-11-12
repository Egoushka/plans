# 0001. Checked plans

Status: accepted

## Context

An agent that calls paid or slow tools can loop or fan out without limit. Runtime guards stop it after the money is spent. We want a plan to be rejected before it runs when it cannot be shown to stay inside the budget it declares.

## Decision

Plans are written in a small language (`docs/plan-language.md`) whose every construct has a bound the checker can compute.

- **Tools are declared** with a cost, and a `budget` block caps each tool's calls and the total cost. The checker proves per-tool and total upper bounds and rejects a plan that exceeds a budget.
- **`for x in 0..N`** has a literal bound. **`for x in items max N`** needs `max`; the runtime truncates at `N`.
- **`while cond fuel K`** requires `fuel`. A `while` without it is rejected, because the loop cannot be bounded.
- **Recursion is rejected**, direct or mutual. Function bodies are checked once and the result reused at each call site.
- **`if` is bounded per path.** The checker keeps the set of non-dominated call vectors, one per path through the plan, and the total is the cost of the worst path. The older rule, an elementwise maximum per tool across branches, overstates plans whose branches exclude each other (`examples/exclusive.plan`: 33 by that rule, 21 by paths). Past 64 paths the checker merges to an elementwise maximum, which stays sound but looser.

## Alternatives considered

- **Corvid** is the closest prior art and **Ingot** the second closest. Neither was adopted: this repo keeps the whole checker small enough to read in one sitting and tied to the cost-and-budget model above. A detailed feature comparison has not been done.
- **Runtime-only limits.** Rejected: they fail after the cost is incurred and give no answer before a run.

## Check line

Every example under `examples/` passes `plans check`, and every one under `examples/bad/` is rejected with its stated reason. `tests/plans.rs` walks both directories, so CI runs this check.

## Stop line

An accepted plan that exceeds a declared budget at run time. That is a checker bug: stop adding language features until it is fixed.
