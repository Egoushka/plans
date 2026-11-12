use crate::lexer::{fail, lex, PlanError, Tok};

type R<T> = Result<T, PlanError>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Add,
    Sub,
    Lt,
    Eq,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64),
    Bool(bool),
    Var(String, usize),
    Bin(Op, Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    /// A call to a tool or a function (resolved by the checker); carries its line.
    Call(String, Vec<Expr>, usize),
    /// A literal list, e.g. `[1, 2, 3]`.
    List(Vec<Expr>),
}

#[derive(Debug, Clone)]
pub enum Kind {
    Let(String, Expr),
    Expr(Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    For(String, u64, Vec<Stmt>),
    /// `for x in items max N`: `max` is optional in the grammar so the checker rejects its absence.
    ForList(String, Expr, Option<u64>, Vec<Stmt>),
    /// `fuel` is optional in the grammar so the checker, not the parser, rejects its absence.
    While(Expr, Option<u64>, Vec<Stmt>),
    Return(Expr),
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub line: usize,
    pub kind: Kind,
}

#[derive(Debug)]
pub struct ToolDecl {
    pub name: String,
    pub cost: u64,
    /// `returns list`: the tool result is a list that a `for ... max` can iterate.
    pub returns_list: bool,
    pub line: usize,
}

#[derive(Debug)]
pub struct FnDecl {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
    pub line: usize,
}

#[derive(Debug, Default)]
pub struct Budget {
    pub total: Option<(u64, usize)>,
    pub per_tool: Vec<(String, u64, usize)>,
}

#[derive(Debug, Default)]
pub struct Program {
    pub tools: Vec<ToolDecl>,
    pub fns: Vec<FnDecl>,
    pub plan: Option<Vec<Stmt>>,
    pub budget: Budget,
}

const KEYWORDS: [&str; 17] = [
    "tool", "cost", "fn", "plan", "budget", "let", "if", "else", "for", "in", "while", "fuel",
    "return", "true", "false", "max", "returns",
];

struct Parser {
    t: Vec<(Tok, usize)>,
    i: usize,
}

impl Parser {
    fn line(&self) -> usize {
        self.t.get(self.i).or(self.t.last()).map_or(1, |x| x.1)
    }
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i).map(|x| &x.0)
    }
    fn is_sym(&self, s: &str) -> bool {
        matches!(self.peek(), Some(Tok::Sym(x)) if *x == s)
    }
    fn is_kw(&self, s: &str) -> bool {
        matches!(self.peek(), Some(Tok::Id(x)) if x == s)
    }
    fn sym(&mut self, s: &str) -> R<()> {
        if self.is_sym(s) {
            self.i += 1;
            Ok(())
        } else {
            fail(self.line(), format!("expected `{s}`"))
        }
    }
    fn kw(&mut self, s: &str) -> R<()> {
        if self.is_kw(s) {
            self.i += 1;
            Ok(())
        } else {
            fail(self.line(), format!("expected `{s}`"))
        }
    }
    fn name(&mut self) -> R<String> {
        match self.peek().cloned() {
            Some(Tok::Id(x)) if !KEYWORDS.contains(&x.as_str()) => {
                self.i += 1;
                Ok(x)
            }
            _ => fail(self.line(), "expected a name"),
        }
    }
    fn num(&mut self) -> R<u64> {
        match self.peek() {
            Some(Tok::Num(n)) => {
                let n = *n as u64;
                self.i += 1;
                Ok(n)
            }
            _ => fail(self.line(), "expected an integer literal"),
        }
    }

    fn program(&mut self) -> R<Program> {
        let mut p = Program::default();
        while let Some(tok) = self.peek().cloned() {
            let line = self.line();
            match tok {
                Tok::Id(k) if k == "tool" => {
                    self.i += 1;
                    let name = self.name()?;
                    self.kw("cost")?;
                    let cost = self.num()?;
                    let returns_list = self.is_kw("returns");
                    if returns_list {
                        self.i += 1;
                        self.kw("list")?;
                    }
                    self.sym(";")?;
                    p.tools.push(ToolDecl { name, cost, returns_list, line });
                }
                Tok::Id(k) if k == "fn" => {
                    self.i += 1;
                    let name = self.name()?;
                    self.sym("(")?;
                    let mut params = Vec::new();
                    while !self.is_sym(")") {
                        if !params.is_empty() {
                            self.sym(",")?;
                        }
                        params.push(self.name()?);
                    }
                    self.sym(")")?;
                    let body = self.block()?;
                    p.fns.push(FnDecl { name, params, body, line });
                }
                Tok::Id(k) if k == "plan" => {
                    self.i += 1;
                    if p.plan.is_some() {
                        return fail(line, "a second `plan` block");
                    }
                    p.plan = Some(self.block()?);
                }
                Tok::Id(k) if k == "budget" => {
                    self.i += 1;
                    self.sym("{")?;
                    while !self.is_sym("}") {
                        let line = self.line();
                        let name = match self.peek().cloned() {
                            Some(Tok::Id(x)) => {
                                self.i += 1;
                                x
                            }
                            _ => return fail(line, "expected `total` or a tool name"),
                        };
                        let n = self.num()?;
                        self.sym(";")?;
                        if name == "total" {
                            p.budget.total = Some((n, line));
                        } else {
                            p.budget.per_tool.push((name, n, line));
                        }
                    }
                    self.sym("}")?;
                }
                _ => return fail(line, "expected `tool`, `fn`, `plan` or `budget`"),
            }
        }
        Ok(p)
    }

    fn block(&mut self) -> R<Vec<Stmt>> {
        self.sym("{")?;
        let mut out = Vec::new();
        while !self.is_sym("}") {
            if self.peek().is_none() {
                return fail(self.line(), "unclosed `{`");
            }
            out.push(self.stmt()?);
        }
        self.sym("}")?;
        Ok(out)
    }

    fn stmt(&mut self) -> R<Stmt> {
        let line = self.line();
        let kind = if self.is_kw("let") {
            self.i += 1;
            let n = self.name()?;
            self.sym("=")?;
            let e = self.expr()?;
            self.sym(";")?;
            Kind::Let(n, e)
        } else if self.is_kw("if") {
            self.i += 1;
            let c = self.expr()?;
            let t = self.block()?;
            let e = if self.is_kw("else") {
                self.i += 1;
                self.block()?
            } else {
                Vec::new()
            };
            Kind::If(c, t, e)
        } else if self.is_kw("for") {
            self.i += 1;
            let v = self.name()?;
            self.kw("in")?;
            let is_range = matches!(self.peek(), Some(Tok::Num(_)))
                && matches!(self.t.get(self.i + 1), Some((Tok::Sym(".."), _)));
            if is_range {
                if self.num()? != 0 {
                    return fail(line, "ranges must start at 0");
                }
                self.sym("..")?;
                let n = self.num()?;
                Kind::For(v, n, self.block()?)
            } else {
                let items = self.expr()?;
                let max = if self.is_kw("max") {
                    self.i += 1;
                    Some(self.num()?)
                } else {
                    None
                };
                Kind::ForList(v, items, max, self.block()?)
            }
        } else if self.is_kw("while") {
            self.i += 1;
            let c = self.expr()?;
            let fuel = if self.is_kw("fuel") {
                self.i += 1;
                Some(self.num()?)
            } else {
                None
            };
            Kind::While(c, fuel, self.block()?)
        } else if self.is_kw("return") {
            self.i += 1;
            let e = self.expr()?;
            self.sym(";")?;
            Kind::Return(e)
        } else {
            let e = self.expr()?;
            self.sym(";")?;
            Kind::Expr(e)
        };
        Ok(Stmt { line, kind })
    }

    // expr := sum (('<' | '==') sum)?   sum := unary (('+' | '-') unary)*
    fn expr(&mut self) -> R<Expr> {
        let l = self.sum()?;
        let op = if self.is_sym("<") {
            Op::Lt
        } else if self.is_sym("==") {
            Op::Eq
        } else {
            return Ok(l);
        };
        self.i += 1;
        let r = self.sum()?;
        Ok(Expr::Bin(op, Box::new(l), Box::new(r)))
    }

    fn sum(&mut self) -> R<Expr> {
        let mut l = self.unary()?;
        loop {
            let op = if self.is_sym("+") {
                Op::Add
            } else if self.is_sym("-") {
                Op::Sub
            } else {
                return Ok(l);
            };
            self.i += 1;
            let r = self.unary()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
    }

    fn unary(&mut self) -> R<Expr> {
        if self.is_sym("!") {
            self.i += 1;
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        let line = self.line();
        match self.peek().cloned() {
            Some(Tok::Num(n)) => {
                self.i += 1;
                Ok(Expr::Int(n))
            }
            Some(Tok::Id(x)) if x == "true" || x == "false" => {
                self.i += 1;
                Ok(Expr::Bool(x == "true"))
            }
            Some(Tok::Sym("[")) => {
                self.i += 1;
                let mut items = Vec::new();
                while !self.is_sym("]") {
                    if !items.is_empty() {
                        self.sym(",")?;
                    }
                    items.push(self.expr()?);
                }
                self.sym("]")?;
                Ok(Expr::List(items))
            }
            Some(Tok::Sym("(")) => {
                self.i += 1;
                let e = self.expr()?;
                self.sym(")")?;
                Ok(e)
            }
            Some(Tok::Id(_)) => {
                let name = self.name()?;
                if !self.is_sym("(") {
                    return Ok(Expr::Var(name, line));
                }
                self.i += 1;
                let mut args = Vec::new();
                while !self.is_sym(")") {
                    if !args.is_empty() {
                        self.sym(",")?;
                    }
                    args.push(self.expr()?);
                }
                self.sym(")")?;
                Ok(Expr::Call(name, args, line))
            }
            _ => fail(line, "expected an expression"),
        }
    }
}

pub fn parse(src: &str) -> R<Program> {
    Parser { t: lex(src)?, i: 0 }.program()
}
