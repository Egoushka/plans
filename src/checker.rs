//! The checker: computes an upper bound on tool calls for every plan, or rejects it.
use crate::lexer::{fail, PlanError};
use crate::parser::{Expr, FnDecl, Kind, Program, Stmt};
use std::collections::{BTreeMap, HashMap, HashSet};

type R<T> = Result<T, PlanError>;

/// Upper bound on how many times each tool is called.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bound {
    pub calls: BTreeMap<String, u64>,
}

impl Bound {
    fn one(tool: &str) -> Bound {
        Bound { calls: BTreeMap::from([(tool.to_string(), 1)]) }
    }
    /// Sequence: calls add up.
    fn add(&mut self, o: &Bound) {
        for (t, n) in &o.calls {
            let e = self.calls.entry(t.clone()).or_insert(0);
            *e = e.saturating_add(*n);
        }
    }
    /// Loop: the body runs at most `k` times.
    fn scale(&self, k: u64) -> Bound {
        let calls = self.calls.iter().map(|(t, n)| (t.clone(), n.saturating_mul(k))).collect();
        Bound { calls }
    }
    /// Per tool, the larger of the two sides.
    fn max(&self, o: &Bound) -> Bound {
        let mut r = self.clone();
        for (t, n) in &o.calls {
            let e = r.calls.entry(t.clone()).or_insert(0);
            *e = (*e).max(*n);
        }
        r
    }
    /// `self` is at least `o` for every tool.
    fn dominates(&self, o: &Bound) -> bool {
        o.calls.iter().all(|(t, n)| self.calls_of(t) >= *n)
    }
    pub fn calls_of(&self, tool: &str) -> u64 {
        self.calls.get(tool).copied().unwrap_or(0)
    }
    pub fn cost(&self, costs: &BTreeMap<String, u64>) -> u64 {
        self.calls.iter().fold(0u64, |a, (t, n)| a.saturating_add(n.saturating_mul(costs[t])))
    }
}

/// One complete path through the plan: its own per-tool call counts and the branch choices that lead to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    pub calls: Bound,
    pub choices: Vec<String>,
}

impl Path {
    pub fn cost(&self, costs: &BTreeMap<String, u64>) -> u64 {
        self.calls.cost(costs)
    }
}

/// Past this many non-dominated paths the set is merged into one elementwise-max path (sound, looser).
const MAX_PATHS: usize = 64;

/// The set of non-dominated per-path cost vectors of a piece of plan.
#[derive(Debug, Clone)]
struct Paths(Vec<Path>);

impl Paths {
    fn none() -> Paths {
        Paths(vec![Path { calls: Bound::default(), choices: vec![] }])
    }
    fn one(tool: &str) -> Paths {
        Paths(vec![Path { calls: Bound::one(tool), choices: vec![] }])
    }
    /// Merge into a single path whose vector is the elementwise max: the old rule.
    fn collapse(&self, note: Option<&str>) -> Paths {
        let calls = self.0.iter().fold(Bound::default(), |a, p| a.max(&p.calls));
        Paths(vec![Path { calls, choices: note.map(|n| vec![n.to_string()]).unwrap_or_default() }])
    }
    /// Drop dominated paths (and exact duplicates, keeping the first); merge if too many remain.
    fn norm(self) -> Paths {
        let v = &self.0;
        let keep: Vec<Path> = (0..v.len())
            .filter(|&i| {
                !(0..v.len()).any(|j| {
                    j != i && v[j].calls.dominates(&v[i].calls) && (j < i || !v[i].calls.dominates(&v[j].calls))
                })
            })
            .map(|i| v[i].clone())
            .collect();
        let out = Paths(keep);
        if out.0.len() > MAX_PATHS {
            return out.collapse(Some("(paths merged: too many to track)"));
        }
        out
    }
    /// Sequence: every path of `self` followed by every path of `o`.
    fn seq(&self, o: &Paths) -> Paths {
        let mut out = Vec::new();
        for a in &self.0 {
            for b in &o.0 {
                let mut calls = a.calls.clone();
                calls.add(&b.calls);
                out.push(Path { calls, choices: [a.choices.clone(), b.choices.clone()].concat() });
            }
        }
        Paths(out).norm()
    }
    /// Loop: the body runs at most `k` times. Counting every iteration on the same path is sound for
    /// totals and per-tool maxima: cost is linear, so mixing paths across iterations never beats the best pure one.
    fn scale(&self, k: u64) -> Paths {
        let v = self.0.iter().map(|p| {
            let choices = if k == 1 { p.choices.clone() } else { p.choices.iter().map(|c| format!("{c} x{k}")).collect() };
            Path { calls: p.calls.scale(k), choices }
        });
        Paths(v.collect()).norm()
    }
    fn tag(mut self, label: String) -> Paths {
        for p in &mut self.0 {
            p.choices.insert(0, label.clone());
        }
        self
    }
}

#[derive(Debug)]
pub struct Checked {
    /// Per tool, the most calls any path makes.
    pub bound: Bound,
    /// Cost of the worst path.
    pub total: u64,
    /// What the old rule (elementwise max at every `if`) gives for the same plan.
    pub legacy_total: u64,
    pub paths: Vec<Path>,
    /// Index into `paths` of the most expensive one.
    pub worst: usize,
    pub costs: BTreeMap<String, u64>,
}

impl Checked {
    pub fn worst_path(&self) -> &Path {
        &self.paths[self.worst]
    }
}

struct Checker<'a> {
    tools: BTreeMap<String, u64>,
    fns: HashMap<&'a str, &'a FnDecl>,
    done: HashMap<String, Paths>,
    stack: Vec<String>,
    /// Old behaviour: collapse both sides of every `if` to one elementwise-max path.
    legacy: bool,
}

type Defined = HashSet<String>;

impl<'a> Checker<'a> {
    /// Bound of a function body; callees are computed first (memoised). A cycle is rejected.
    fn fn_bound(&mut self, name: &str, line: usize) -> R<Paths> {
        if let Some(b) = self.done.get(name) {
            return Ok(b.clone());
        }
        if self.stack.iter().any(|s| s == name) {
            let path = [self.stack.clone(), vec![name.to_string()]].concat().join(" -> ");
            return fail(line, format!("recursion is not allowed ({path}); no bound can be proven"));
        }
        let f = self.fns[name];
        self.stack.push(name.to_string());
        let mut defined: Defined = f.params.iter().cloned().collect();
        let b = self.block(&f.body, &mut defined)?;
        self.stack.pop();
        self.done.insert(name.to_string(), b.clone());
        Ok(b)
    }

    fn block(&mut self, stmts: &[Stmt], defined: &mut Defined) -> R<Paths> {
        let mut total = Paths::none();
        for s in stmts {
            let b = self.stmt(s, defined)?;
            total = total.seq(&b);
        }
        Ok(total)
    }

    fn stmt(&mut self, s: &Stmt, defined: &mut Defined) -> R<Paths> {
        match &s.kind {
            Kind::Let(n, e) => {
                let b = self.expr(e, defined)?;
                defined.insert(n.clone());
                Ok(b)
            }
            Kind::Expr(e) | Kind::Return(e) => self.expr(e, defined),
            Kind::If(c, t, e) => {
                let b = self.expr(c, defined)?;
                let (mut dt, mut de) = (defined.clone(), defined.clone());
                let bt = self.block(t, &mut dt)?.tag(format!("line {} then", s.line));
                let be = self.block(e, &mut de)?.tag(format!("line {} else", s.line));
                let both = Paths([bt.0, be.0].concat()).norm();
                let b = b.seq(&if self.legacy { both.collapse(None) } else { both });
                // only names defined on both paths are safe to use afterwards
                *defined = dt.intersection(&de).cloned().collect();
                Ok(b)
            }
            Kind::For(v, n, body) => {
                let mut inner = defined.clone();
                inner.insert(v.clone());
                Ok(self.block(body, &mut inner)?.scale(*n))
            }
            Kind::ForList(v, items, max, body) => {
                let Some(n) = max else {
                    return fail(s.line, format!(
                        "`for {v} in ...` over a list has no bound: add `max N`, e.g. `for {v} in ... max 20 {{ ... }}`"
                    ));
                };
                // the list is evaluated once, then the body runs at most `max` times
                let b = self.expr(items, defined)?;
                let mut inner = defined.clone();
                inner.insert(v.clone());
                Ok(b.seq(&self.block(body, &mut inner)?.scale(*n)))
            }
            Kind::While(c, fuel, body) => {
                let Some(k) = fuel else {
                    return fail(s.line, "`while` without `fuel`: the loop cannot be bounded");
                };
                // each iteration evaluates the condition once, then the body
                let b = self.expr(c, defined)?;
                Ok(b.seq(&self.block(body, &mut defined.clone())?).scale(*k))
            }
        }
    }

    fn expr(&mut self, e: &Expr, defined: &Defined) -> R<Paths> {
        match e {
            Expr::Int(_) | Expr::Bool(_) => Ok(Paths::none()),
            Expr::List(items) => {
                let mut r = Paths::none();
                for e in items {
                    r = r.seq(&self.expr(e, defined)?);
                }
                Ok(r)
            }
            Expr::Var(v, line) => {
                if defined.contains(v) {
                    Ok(Paths::none())
                } else {
                    fail(*line, format!("`{v}` is not defined here"))
                }
            }
            Expr::Not(a) => self.expr(a, defined),
            Expr::Bin(_, a, b) => {
                let r = self.expr(a, defined)?;
                Ok(r.seq(&self.expr(b, defined)?))
            }
            Expr::Call(name, args, line) => {
                let mut r = Paths::none();
                for a in args {
                    r = r.seq(&self.expr(a, defined)?);
                }
                if self.tools.contains_key(name) {
                    r = r.seq(&Paths::one(name));
                } else if let Some(f) = self.fns.get(name.as_str()) {
                    if f.params.len() != args.len() {
                        return fail(*line, format!("`{name}` takes {} argument(s), got {}", f.params.len(), args.len()));
                    }
                    r = r.seq(&self.fn_bound(name, *line)?);
                } else {
                    return fail(*line, format!("unknown tool or function `{name}`"));
                }
                Ok(r)
            }
        }
    }
}

/// Check a plan with either the path-set rule or the old elementwise-max rule; returns the plan's paths.
fn analyze(p: &Program, legacy: bool) -> R<(Paths, BTreeMap<String, u64>)> {
    let mut c = Checker { tools: BTreeMap::new(), fns: HashMap::new(), done: HashMap::new(), stack: vec![], legacy };
    for t in &p.tools {
        if t.name == "total" || c.tools.insert(t.name.clone(), t.cost).is_some() {
            return fail(t.line, format!("tool name `{}` is reserved or declared twice", t.name));
        }
    }
    for f in &p.fns {
        if c.tools.contains_key(&f.name) || c.fns.insert(&f.name, f).is_some() {
            return fail(f.line, format!("`{}` is declared twice", f.name));
        }
    }
    for f in &p.fns {
        c.fn_bound(&f.name, f.line)?; // also checks functions the plan never calls
    }
    let Some(plan) = &p.plan else { return fail(1, "no `plan` block") };
    let paths = c.block(plan, &mut Defined::new())?;
    Ok((paths, c.tools))
}

pub fn check(p: &Program) -> R<Checked> {
    let (paths, costs) = analyze(p, false)?;
    let paths = paths.0;
    let bound = paths.iter().fold(Bound::default(), |a, x| a.max(&x.calls));
    let (worst, total) = paths
        .iter()
        .enumerate()
        .map(|(i, x)| (i, x.cost(&costs)))
        .fold((0, 0), |best, cur| if cur.1 > best.1 { cur } else { best });

    for (tool, limit, line) in &p.budget.per_tool {
        if !costs.contains_key(tool) {
            return fail(*line, format!("budget names unknown tool `{tool}`"));
        }
        let n = bound.calls_of(tool);
        if n > *limit {
            return fail(*line, format!("budget exceeded: `{tool}` may be called up to {n} times, budget allows {limit}"));
        }
    }
    if let Some((limit, line)) = p.budget.total {
        if total > limit {
            return fail(line, format!("budget exceeded: total cost may reach {total}, budget allows {limit}"));
        }
    }
    let legacy_total = analyze(p, true)?.0 .0[0].cost(&costs);
    Ok(Checked { bound, total, legacy_total, paths, worst, costs })
}
