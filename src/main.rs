use checked_plans::checker::Checked;
use checked_plans::interp::{self, Report, Val};
use checked_plans::load;
use std::process::ExitCode;

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

fn val(v: &Val) -> String {
    match v {
        Val::Int(n) => n.to_string(),
        Val::Bool(b) => b.to_string(),
        Val::List(l) => format!("list of {}", l.len()),
    }
}

fn join(m: impl Iterator<Item = (String, u64)>) -> String {
    let s: Vec<String> = m.map(|(t, n)| format!("{t} {n}")).collect();
    if s.is_empty() {
        "-".into()
    } else {
        s.join(", ")
    }
}

fn print_check(c: &Checked, limits: &checked_plans::parser::Budget) {
    for (t, n) in &c.bound.calls {
        let lim = limits
            .per_tool
            .iter()
            .find(|x| &x.0 == t)
            .map_or("unlimited".into(), |x| x.1.to_string());
        println!("  {t:<12} bound {n:<4} budget {lim}");
    }
    let lim = limits.total.map_or("unlimited".into(), |x| x.0.to_string());
    println!("  {:<12} bound {:<4} budget {lim}", "total cost", c.total);
    if c.paths.len() > 1 {
        println!("  {} paths after pruning dominated ones:", c.paths.len());
        for p in &c.paths {
            let calls = join(p.calls.calls.iter().map(|(t, n)| (t.clone(), *n)));
            println!(
                "    cost {:<4} {calls}  [{}]",
                p.cost(&c.costs),
                choices(&p.choices)
            );
        }
    }
    if !c.worst_path().choices.is_empty() {
        println!(
            "  worst path   {} (cost {})",
            choices(&c.worst_path().choices),
            c.total
        );
    }
    if c.legacy_total != c.total {
        println!(
            "  (elementwise-max rule would have said total {})",
            c.legacy_total
        );
    }
}

fn choices(c: &[String]) -> String {
    if c.is_empty() {
        "-".into()
    } else {
        c.join(", ")
    }
}

fn print_run(c: &Checked, r: &Report) {
    for (t, n) in &c.bound.calls {
        let actual = r.calls.get(t).copied().unwrap_or(0);
        println!("  {t:<12} bound {n:<4} actual {actual}");
    }
    println!(
        "  {:<12} bound {:<4} actual {}",
        "total cost", c.total, r.cost
    );
    for t in &r.truncated {
        println!(
            "  loop at line {} truncated: list had {} items, max {}",
            t.line, t.len, t.max
        );
    }
    if let Some(v) = &r.result {
        println!("  plan returned {}", val(v));
    }
}

fn table(paths: &[String]) -> Result<(), String> {
    println!("| plan | per-tool bound | per-tool actual | total bound | total actual | worst path | check |");
    println!("|---|---|---|---|---|---|---|");
    for p in paths {
        let name = p.rsplit('/').next().unwrap_or(p);
        match load(&read(p)?) {
            Ok((prog, c)) => {
                let r = interp::run(&prog).map_err(|e| format!("{p}: {e}"))?;
                let bound = join(c.bound.calls.iter().map(|(t, n)| (t.clone(), *n)));
                let actual = join(
                    c.bound
                        .calls
                        .keys()
                        .map(|t| (t.clone(), r.calls.get(t).copied().unwrap_or(0))),
                );
                let mut check = "pass".to_string();
                for t in &r.truncated {
                    check += &format!(
                        ", loop at line {} truncated ({} items, max {})",
                        t.line, t.len, t.max
                    );
                }
                println!(
                    "| {name} | {bound} | {actual} | {} | {} | {} | {check} |",
                    c.total,
                    r.cost,
                    choices(&c.worst_path().choices)
                );
            }
            Err(e) => println!("| {name} | - | - | - | - | - | REJECTED: {e} |"),
        }
    }
    Ok(())
}

fn go(args: &[String]) -> Result<ExitCode, String> {
    match args {
        [cmd, path] if cmd == "check" => match load(&read(path)?) {
            Ok((prog, c)) => {
                println!("{path}: OK, proven within budget");
                print_check(&c, &prog.budget);
                Ok(ExitCode::SUCCESS)
            }
            Err(e) => {
                println!("{path}: REJECTED at {e}");
                Ok(ExitCode::from(1))
            }
        },
        [cmd, path] if cmd == "run" => match load(&read(path)?) {
            Ok((prog, c)) => {
                let r = interp::run(&prog).map_err(|e| format!("{path}: runtime error at {e}"))?;
                println!("{path}: ran");
                print_run(&c, &r);
                Ok(ExitCode::SUCCESS)
            }
            Err(e) => {
                println!("{path}: REJECTED at {e}; refusing to run");
                Ok(ExitCode::from(1))
            }
        },
        [cmd, paths @ ..] if cmd == "table" && !paths.is_empty() => {
            table(paths).map(|_| ExitCode::SUCCESS)
        }
        _ => Err("usage: plans check <file> | plans run <file> | plans table <files...>".into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    go(&args).unwrap_or_else(|e| {
        eprintln!("{e}");
        ExitCode::from(2)
    })
}
