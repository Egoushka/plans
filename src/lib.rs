//! "Checked plans": a toy language whose checker proves a static upper bound
//! on tool calls and cost before the plan is allowed to run.
pub mod checker;
pub mod interp;
pub mod lexer;
pub mod parser;

use checker::Checked;
use lexer::PlanError;
use parser::Program;

/// Parse and check a source text. Only a plan that passes can be run.
pub fn load(src: &str) -> Result<(Program, Checked), PlanError> {
    let prog = parser::parse(src)?;
    let checked = checker::check(&prog)?;
    Ok((prog, checked))
}
