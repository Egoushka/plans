use checked_plans::checker::Checked;
use checked_plans::{interp, load};
use std::fs;

const TOOLS: &str = "tool a cost 2; tool b cost 5;";

fn ok(body: &str) -> Checked {
    load(&format!("{TOOLS} {body}"))
        .unwrap_or_else(|e| panic!("should pass: {e}"))
        .1
}
fn err(body: &str) -> String {
    match load(&format!("{TOOLS} {body}")) {
        Ok(_) => panic!("should be rejected"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn sequence_sums() {
    let c = ok("plan { a(1); a(2); b(3); }");
    assert_eq!(
        (c.bound.calls_of("a"), c.bound.calls_of("b"), c.total),
        (2, 1, 9)
    );
}

#[test]
fn if_takes_max_of_branches() {
    let c = ok("plan { if true { a(1); a(2); } else { a(1); b(1); } }");
    assert_eq!((c.bound.calls_of("a"), c.bound.calls_of("b")), (2, 1));
}

#[test]
fn for_multiplies_body() {
    let c = ok("plan { for i in 0..4 { a(i); b(i); } }");
    assert_eq!(
        (c.bound.calls_of("a"), c.bound.calls_of("b"), c.total),
        (4, 4, 28)
    );
}

#[test]
fn nested_loops_multiply() {
    let c = ok("plan { for i in 0..3 { for j in 0..5 { a(j); } } }");
    assert_eq!(c.bound.calls_of("a"), 15);
}

#[test]
fn while_multiplies_by_fuel_and_counts_condition() {
    let c = ok("plan { let x = 0; while x < 3 fuel 5 { let x = a(0); } }");
    assert_eq!(c.bound.calls_of("a"), 5);
    let c = ok("plan { let x = 0; while a(x) < 3 fuel 5 { let x = b(0); } }");
    assert_eq!((c.bound.calls_of("a"), c.bound.calls_of("b")), (5, 5));
}

#[test]
fn while_without_fuel_is_rejected() {
    assert!(err("plan { while true { a(1); } }").contains("without `fuel`"));
}

#[test]
fn function_call_costs_callee_bound() {
    let c = ok("fn f(x) { a(x); return b(x); } fn g(x) { f(x); return f(x); } plan { for i in 0..3 { g(i); } }");
    assert_eq!((c.bound.calls_of("a"), c.bound.calls_of("b")), (6, 6));
}

#[test]
fn direct_and_mutual_recursion_rejected() {
    assert!(err("fn f(x) { return f(x); } plan { f(1); }").contains("recursion"));
    assert!(
        err("fn f(x) { return g(x); } fn g(x) { return f(x); } plan { f(1); }")
            .contains("recursion")
    );
}

#[test]
fn budget_violations_report_line_and_bound() {
    let e = err("budget {\n a 3;\n} plan { for i in 0..4 { a(i); } }");
    assert!(
        e.starts_with("line 2:") && e.contains("up to 4 times"),
        "{e}"
    );
    let e = err("budget { total 10; } plan { a(1); b(1); b(1); }");
    assert!(e.contains("total cost may reach 12"), "{e}");
}

#[test]
fn unknown_names_and_bad_arity_rejected() {
    assert!(err("plan { nope(1); }").contains("unknown"));
    assert!(err("fn f(x) { return x; } plan { f(1, 2); }").contains("argument"));
    assert!(err("plan { return y; }").contains("not defined"));
}

#[test]
fn fuel_stops_loop_at_runtime() {
    let (p, c) = load(&format!("{TOOLS} plan {{ while true fuel 4 {{ a(1); }} }}")).unwrap();
    let r = interp::run(&p).unwrap();
    assert_eq!(r.calls["a"], 4);
    assert_eq!(c.bound.calls_of("a"), 4);
}

fn files(dir: &str) -> Vec<String> {
    let d = format!("{}/examples/{dir}", env!("CARGO_MANIFEST_DIR"));
    let mut v: Vec<String> = fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap().path().display().to_string())
        .filter(|p| p.ends_with(".plan"))
        .collect();
    v.sort();
    v
}

#[test]
fn bad_examples_are_rejected_at_check_time() {
    let bad = files("bad");
    assert_eq!(bad.len(), 5);
    for f in bad {
        let reason = match f.rsplit('/').next().unwrap() {
            "a_no_fuel.plan" => "without `fuel`",
            "b_loop_over_budget.plan" => "budget exceeded: `fetch` may be called up to 4 times",
            "c_recursion.plan" => "recursion",
            "d_total_cost.plan" => "total cost may reach 120",
            "e_list_no_max.plan" => "add `max N`",
            other => panic!("{other}: no expected reason recorded"),
        };
        let err = load(&fs::read_to_string(&f).unwrap())
            .err()
            .unwrap_or_else(|| panic!("{f} should be rejected"));
        assert!(
            err.to_string().contains(reason),
            "{f}: expected `{reason}`, got `{err}`"
        );
    }
}

#[test]
fn good_examples_run_within_their_bound() {
    let good = files("");
    assert_eq!(good.len(), 7);
    for f in good {
        let (p, c) = load(&fs::read_to_string(&f).unwrap()).unwrap_or_else(|e| panic!("{f}: {e}"));
        let r = interp::run(&p).unwrap_or_else(|e| panic!("{f}: {e}"));
        for (t, n) in &r.calls {
            assert!(*n <= c.bound.calls_of(t), "{f}: {t} actual {n} > bound");
        }
        assert!(
            r.cost <= c.total,
            "{f}: cost {} > bound {}",
            r.cost,
            c.total
        );
    }
}

const LIST_TOOLS: &str = "tool search cost 3 returns list; tool fetch cost 2;";

#[test]
fn for_max_is_accepted_with_proven_bound_and_truncates_at_runtime() {
    let src = format!(
        "{LIST_TOOLS} plan {{ let hits = search(1); for h in hits max 10 {{ fetch(h); }} }}"
    );
    let (p, c) = load(&src).unwrap();
    assert_eq!(
        (
            c.bound.calls_of("search"),
            c.bound.calls_of("fetch"),
            c.total
        ),
        (1, 10, 23)
    );
    let r = interp::run(&p).unwrap();
    assert_eq!(r.calls["fetch"], 10); // the fake search returns 25 items
    assert_eq!(r.truncated.len(), 1);
    assert_eq!((r.truncated[0].len, r.truncated[0].max), (25, 10));
    assert!(r.cost <= c.total);
}

#[test]
fn for_max_over_short_list_does_not_truncate() {
    let src = format!("{LIST_TOOLS} plan {{ for x in [1, 2, 3] max 5 {{ fetch(x); }} }}");
    let (p, c) = load(&src).unwrap();
    assert_eq!(c.bound.calls_of("fetch"), 5);
    let r = interp::run(&p).unwrap();
    assert_eq!(r.calls["fetch"], 3);
    assert!(r.truncated.is_empty());
}

#[test]
fn for_max_multiplies_in_nested_loops() {
    let c = ok("plan { for i in 0..3 { for x in [1, 2] max 4 { a(x); } } }");
    assert_eq!(c.bound.calls_of("a"), 12);
}

#[test]
fn list_loop_without_max_is_rejected_with_the_fix() {
    let e = match load(&format!(
        "{LIST_TOOLS} plan {{ let hits = search(1);\n for h in hits {{ fetch(h); }} }}"
    )) {
        Ok(_) => panic!("should be rejected"),
        Err(e) => e.to_string(),
    };
    assert!(e.starts_with("line 2:") && e.contains("add `max N`"), "{e}");
    assert!(err("plan { for x in [1, 2] { a(x); } }").contains("max"));
}

#[test]
fn for_over_a_non_list_fails_at_runtime_not_silently() {
    let (p, _) = load(&format!("{TOOLS} plan {{ for x in 3 max 2 {{ a(x); }} }}")).unwrap();
    assert!(interp::run(&p).is_err());
}

#[test]
fn if_paths_are_tighter_than_elementwise_max() {
    // x costs 5, y costs 5. First if: {x,x} or {y}. Second if: {y,y} or {x}.
    // Elementwise max: x 2+1, y 1+2 -> 6 calls -> 30. Paths: best is x,x,y,y = 20.
    let c = ok_with(
        "tool x cost 5; tool y cost 5;",
        "plan { if true { x(1); x(1); } else { y(1); } if true { y(1); y(1); } else { x(1); } }",
    );
    assert_eq!(c.legacy_total, 30);
    assert_eq!(c.total, 20);
    assert!(c.total < c.legacy_total);
    // per-tool bounds are still the per-path maxima
    assert_eq!((c.bound.calls_of("x"), c.bound.calls_of("y")), (3, 3));
}

fn ok_with(tools: &str, body: &str) -> Checked {
    load(&format!("{tools} {body}"))
        .unwrap_or_else(|e| panic!("should pass: {e}"))
        .1
}

#[test]
fn worst_path_names_the_branch_choices() {
    let c = ok_with(
        "tool x cost 5; tool y cost 1;",
        "plan {\n if true { x(1); } else { y(1); }\n }",
    );
    assert_eq!(c.worst_path().choices, vec!["line 2 then".to_string()]);
    assert_eq!(c.total, 5);
}

#[test]
fn dominated_paths_are_pruned_and_straight_line_plans_have_one_path() {
    let c = ok("plan { if true { a(1); a(1); } else { a(1); } }");
    assert_eq!(c.paths.len(), 1);
    assert_eq!(c.worst_path().choices, vec!["line 1 then".to_string()]);
    assert!(ok("plan { a(1); }").worst_path().choices.is_empty());
}

#[test]
fn exclusive_example_is_tighter_than_old_rule() {
    let src = fs::read_to_string(format!(
        "{}/examples/exclusive.plan",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let c = load(&src).unwrap().1;
    assert_eq!((c.legacy_total, c.total), (33, 21));
}

#[test]
fn too_many_paths_merge_soundly() {
    let mut body = String::from("plan { ");
    for _ in 0..12 {
        body += "if true { x(1); } else { if true { y(1); } else { z(1); } } ";
    }
    body += "}";
    let c = ok_with("tool x cost 1; tool y cost 2; tool z cost 3;", &body);
    // 12 ifs have C(14,2)=91 non-dominated paths (> 64), so the set was merged after the 10th if
    // (10 of each tool = 60), then 2 more ifs: 60 + 6 (z, z) = 66. The true worst path is 12 z = 36.
    assert_eq!(c.legacy_total, 72);
    assert_eq!(c.total, 66);
    assert!(c.total >= 36 && c.total <= c.legacy_total);
}

#[test]
fn paths_through_loops_and_functions_scale() {
    let c = ok("fn f(i) { if i < 1 { a(i); a(i); } else { b(i); } return 0; } plan { for i in 0..3 { f(i); } }");
    // body paths {a2} or {b1}, scaled by 3: worst is b3 = 15, not a6 + b3 = 27
    assert_eq!(c.total, 15);
    assert_eq!(c.legacy_total, 6 * 2 + 3 * 5);
}
