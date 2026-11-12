//! The interpreter: runs a plan that already passed the checker and counts what really happens.
use crate::lexer::{fail, PlanError};
use crate::parser::{Expr, FnDecl, Kind, Op, Program, Stmt};
use std::collections::{BTreeMap, HashMap};

type R<T> = Result<T, PlanError>;

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Int(i64),
    Bool(bool),
    List(Vec<i64>),
}

/// A `for ... max N` loop that cut its list short.
#[derive(Debug, Clone, PartialEq)]
pub struct Truncation {
    pub line: usize,
    pub len: usize,
    pub max: u64,
}

#[derive(Debug, Default)]
pub struct Report {
    pub calls: BTreeMap<String, u64>,
    pub cost: u64,
    pub result: Option<Val>,
    pub truncated: Vec<Truncation>,
}

type Env = HashMap<String, Val>;

struct Vm<'a> {
    tools: HashMap<&'a str, (u64, bool)>,
    fns: HashMap<&'a str, &'a FnDecl>,
    rep: Report,
    line: usize,
}

impl<'a> Vm<'a> {
    fn block(&mut self, b: &[Stmt], env: &mut Env) -> R<Option<Val>> {
        for s in b {
            if let Some(v) = self.stmt(s, env)? {
                return Ok(Some(v));
            }
        }
        Ok(None)
    }

    // Some(v) means `return v` was executed.
    fn stmt(&mut self, s: &Stmt, env: &mut Env) -> R<Option<Val>> {
        self.line = s.line;
        match &s.kind {
            Kind::Let(n, e) => {
                let v = self.eval(e, env)?;
                env.insert(n.clone(), v); // flat scope: `let` also re-assigns
            }
            Kind::Expr(e) => {
                self.eval(e, env)?;
            }
            Kind::Return(e) => return Ok(Some(self.eval(e, env)?)),
            Kind::If(c, t, e) => {
                let branch = if self.cond(c, env)? { t } else { e };
                return self.block(branch, env);
            }
            Kind::For(v, n, body) => {
                for i in 0..*n {
                    env.insert(v.clone(), Val::Int(i as i64));
                    if let Some(r) = self.block(body, env)? {
                        return Ok(Some(r));
                    }
                }
            }
            Kind::ForList(v, items, max, body) => {
                let line = s.line;
                let Val::List(list) = self.eval(items, env)? else {
                    return fail(line, "`for ... in` needs a list");
                };
                let max = max.unwrap_or(0);
                if list.len() as u64 > max {
                    self.rep.truncated.push(Truncation { line, len: list.len(), max });
                }
                for x in list.into_iter().take(max as usize) {
                    env.insert(v.clone(), Val::Int(x));
                    if let Some(r) = self.block(body, env)? {
                        return Ok(Some(r));
                    }
                }
            }
            Kind::While(c, fuel, body) => {
                // fuel is the semantics: the loop stops after K iterations even if the condition holds
                for _ in 0..fuel.unwrap_or(0) {
                    if !self.cond(c, env)? {
                        break;
                    }
                    if let Some(r) = self.block(body, env)? {
                        return Ok(Some(r));
                    }
                }
            }
        }
        Ok(None)
    }

    fn cond(&mut self, e: &Expr, env: &mut Env) -> R<bool> {
        match self.eval(e, env)? {
            Val::Bool(b) => Ok(b),
            _ => fail(self.line, "condition must be a bool"),
        }
    }

    fn int(&self, v: Val) -> R<i64> {
        match v {
            Val::Int(n) => Ok(n),
            _ => fail(self.line, "expected an integer, found a bool or list"),
        }
    }

    fn eval(&mut self, e: &Expr, env: &mut Env) -> R<Val> {
        match e {
            Expr::Int(n) => Ok(Val::Int(*n)),
            Expr::Bool(b) => Ok(Val::Bool(*b)),
            Expr::Var(v, line) => match env.get(v) {
                Some(x) => Ok(x.clone()),
                None => fail(*line, format!("`{v}` is not defined")),
            },
            Expr::Not(a) => match self.eval(a, env)? {
                Val::Bool(b) => Ok(Val::Bool(!b)),
                _ => fail(self.line, "`!` needs a bool"),
            },
            Expr::Bin(op, a, b) => {
                let (x, y) = (self.eval(a, env)?, self.eval(b, env)?);
                match op {
                    Op::Eq => Ok(Val::Bool(x == y)),
                    Op::Add => Ok(Val::Int(self.int(x)?.wrapping_add(self.int(y)?))),
                    Op::Sub => Ok(Val::Int(self.int(x)?.wrapping_sub(self.int(y)?))),
                    Op::Lt => Ok(Val::Bool(self.int(x)? < self.int(y)?)),
                }
            }
            Expr::List(items) => {
                let mut out = Vec::new();
                for e in items {
                    let v = self.eval(e, env)?;
                    out.push(self.int(v)?);
                }
                Ok(Val::List(out))
            }
            Expr::Call(name, args, _) => {
                let mut vals = Vec::new();
                for a in args {
                    vals.push(self.eval(a, env)?);
                }
                if let Some(&(cost, returns_list)) = self.tools.get(name.as_str()) {
                    // Deterministic fake tool: result = how many times this tool has run + sum of int args.
                    self.rep.cost += cost;
                    let n = self.rep.calls.entry(name.clone()).or_insert(0);
                    *n += 1;
                    let mut r = *n as i64;
                    for v in vals {
                        if let Val::Int(x) = v {
                            r = r.wrapping_add(x);
                        }
                    }
                    if returns_list {
                        // fake list tool: 25 items, so any `max` below 25 visibly truncates
                        return Ok(Val::List((1..=25).map(|k| r.wrapping_add(k)).collect()));
                    }
                    return Ok(Val::Int(r));
                }
                let f = self.fns[name.as_str()];
                let mut local: Env = f.params.iter().cloned().zip(vals).collect();
                let r = self.block(&f.body, &mut local)?;
                Ok(r.unwrap_or(Val::Int(0)))
            }
        }
    }
}

/// Run a plan. The caller must have run `checker::check` first (see `load`).
pub fn run(p: &Program) -> R<Report> {
    let mut vm = Vm {
        tools: p.tools.iter().map(|t| (t.name.as_str(), (t.cost, t.returns_list))).collect(),
        fns: p.fns.iter().map(|f| (f.name.as_str(), f)).collect(),
        rep: Report::default(),
        line: 0,
    };
    let Some(plan) = &p.plan else { return fail(1, "no `plan` block") };
    vm.rep.result = vm.block(plan, &mut Env::new())?;
    Ok(vm.rep)
}
