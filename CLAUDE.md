# CLAUDE.md — plans

Public MIT Rust repository: `checked-plans`, a small language where an agent plan declares the tools it may call with costs and budgets, and a checker proves the bounds before the plan runs. Library `checked_plans`, CLI `plans`.

## Commands

```bash
cargo fmt --check                          # formatting is a gate
cargo clippy --all-targets -- -D warnings  # warnings are errors
cargo test                                 # unit, integration and the examples walk
cargo run -- check examples/retry.plan     # try one plan; also `run` and `table`
```

## Rules

- Every plan under `examples/` must pass `plans check`; every plan under `examples/bad/` must be rejected with the reason `tests/plans.rs` records for it. Adding either kind means updating that test.
- A construct goes into the parser only with a checker rule that bounds it. No loop without a proven bound, no recursion.
- `docs/plan-language.md` describes only what `src/parser.rs` accepts; change both together.
- No MCP tool manifest (milestone 3) until a consumer needs it.
- Actions are pinned by full commit SHA with a `# vX.Y.Z` comment.
- Public repository: no hostnames, IP addresses, key names, home paths or employer names in any file.
- Commits: Conventional Commits.
- Architecturally significant decisions get an ADR under `docs/adr/`, numbered, one per decision.
- No time estimates in docs or commit messages.
