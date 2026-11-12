# The `.plan` language

Everything here is accepted by `src/parser.rs` and judged by `src/checker.rs`. Comments start with `//` and run to the end of the line. Numbers are non-negative integers. Every statement ends with `;` except `if`, `for`, `while` blocks.

A file holds, in any order: `tool` declarations, `fn` definitions, one `plan` block and optional `budget` blocks.

## Tools

```
tool fetch cost 4;
tool search cost 3 returns list;
```

A tool has a name and a cost per call. `returns list` marks a tool whose result is a list that `for ... max` can iterate. Tool names must be unique and `total` is reserved.

## Budget

```
budget { total 30; fetch 5; log 1; }
```

`total` caps the plan's total cost; a tool name caps that tool's calls. A budget entry for an unknown tool is an error. Without a budget entry the bound is still computed and shown as `unlimited`.

## Functions

```
fn load(k) { return read(k) + read(k + 1); }
```

Parameters are comma-separated names. A function may call tools and other functions. Recursion, direct or mutual, is rejected. Calling a function with the wrong number of arguments is an error, and so is declaring a function twice. Tool calls are not checked for argument count.

## Plan

```
plan { ... }
```

Exactly one `plan` block. It is a list of statements.

## Statements

| Statement | Form |
|---|---|
| let | `let x = expr;` (rebinding a name is allowed) |
| expression | `expr;` |
| return | `return expr;` |
| if | `if cond { ... }` or `if cond { ... } else { ... }` |
| counted loop | `for i in 0..N { ... }` (the range must start at 0) |
| list loop | `for x in expr max N { ... }` |
| while | `while cond fuel K { ... }` |

- `for x in expr` without `max` is rejected: the list has no bound. The runtime stops after `N` items.
- `while` without `fuel` is rejected: the loop cannot be bounded. The loop runs at most `K` times.
- After an `if`, only names defined on both branches are defined.
- Using an undefined variable, or calling an unknown tool or function, is an error.

## Expressions

- Integers, `true`, `false`, variables, lists `[a, b]`, parentheses.
- Calls: `name(arg, ...)` for a tool or a function.
- `!x`, `a + b`, `a - b`.
- One comparison, `a < b` or `a == b`. Comparisons do not chain: `a < b < c` is a parse error.

Keywords, which cannot be names: `tool cost fn plan budget let if else for in while fuel return true false max returns`.

## What the checker reports

For an accepted plan: per-tool call bounds, the total cost bound, and for plans with several `if` paths the list of non-dominated paths with the worst one. For a rejected plan: the line and the reason, one of missing `fuel`, missing `max`, recursion, a per-tool or total budget exceeded, an unknown or duplicate name, wrong function argument count, an undefined variable, no `plan` block, or a second `plan` block.

Run it with `plans check <file>`, `plans run <file>` or `plans table <files...>`.
