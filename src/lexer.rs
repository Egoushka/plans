use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Id(String),
    Num(i64),
    Sym(&'static str),
}

#[derive(Debug, Clone)]
pub struct PlanError {
    pub line: usize,
    pub msg: String,
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.msg)
    }
}

pub fn fail<T>(line: usize, msg: impl Into<String>) -> Result<T, PlanError> {
    Err(PlanError {
        line,
        msg: msg.into(),
    })
}

// Two-character symbols come first so they win over their prefixes.
const SYMS: [&str; 15] = [
    "..", "==", "{", "}", "(", ")", "[", "]", ";", ",", "+", "-", "<", "!", "=",
];

pub fn lex(src: &str) -> Result<Vec<(Tok, usize)>, PlanError> {
    let cs: Vec<char> = src.chars().collect();
    let (mut i, mut line, mut out) = (0, 1, Vec::new());
    while i < cs.len() {
        let c = cs[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '/' && cs.get(i + 1) == Some(&'/') {
            while i < cs.len() && cs[i] != '\n' {
                i += 1;
            }
        } else if c.is_ascii_digit() {
            let s = i;
            while i < cs.len() && cs[i].is_ascii_digit() {
                i += 1;
            }
            let text: String = cs[s..i].iter().collect();
            match text.parse() {
                Ok(n) => out.push((Tok::Num(n), line)),
                Err(_) => return fail(line, format!("number `{text}` is too large")),
            }
        } else if c.is_alphabetic() || c == '_' {
            let s = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            out.push((Tok::Id(cs[s..i].iter().collect()), line));
        } else {
            let two: String = cs[i..(i + 2).min(cs.len())].iter().collect();
            match SYMS.iter().copied().find(|s| two.starts_with(*s)) {
                Some(s) => {
                    out.push((Tok::Sym(s), line));
                    i += s.len();
                }
                None => return fail(line, format!("unexpected character `{c}`")),
            }
        }
    }
    Ok(out)
}
