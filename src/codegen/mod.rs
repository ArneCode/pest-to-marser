mod comments;
mod expr_analysis;
mod format;
mod generator;
mod import_needs;
mod naming;

use crate::error::ConvertError;
use crate::normalize::RuleTable;
use crate::scc::{Scc, tarjan_scc};
use crate::specialize::{SpecializationGraph, build_specialization_graph};

#[allow(unused_imports)]
pub use comments::extract_rule_source_comments;
#[allow(unused_imports)]
pub use naming::{bind_var_name, sanitize_ident};

pub struct CodegenOptions {
    pub function_name: String,
    pub source: Option<String>,
    pub emit_comments: bool,
    pub emit_trace: bool,
}

impl Default for CodegenOptions {
    fn default() -> Self {
        Self {
            function_name: "grammar".to_string(),
            source: None,
            emit_comments: true,
            emit_trace: false,
        }
    }
}

pub fn generate_rust(
    table: &RuleTable,
    graph: &SpecializationGraph,
    sccs: &[Scc],
    options: &CodegenOptions,
) -> Result<String, ConvertError> {
    let mut generator = generator::Generator::new(table, graph, sccs, options);
    generator.emit()
}

pub fn prepare_codegen(
    table: &RuleTable,
    entry_rule: &str,
) -> Result<(SpecializationGraph, Vec<Scc>), crate::error::ConvertError> {
    let graph = build_specialization_graph(
        &table.rules,
        entry_rule,
        table.has_whitespace,
        table.has_comment,
    )
    .map_err(|e| {
        e.into_iter()
            .next()
            .unwrap_or(ConvertError::UnknownEntryRule {
                name: entry_rule.to_string(),
            })
    })?;
    let sccs = tarjan_scc(&graph).map_err(|e| {
        e.into_iter()
            .next()
            .unwrap_or(ConvertError::SccTooLarge { size: 13 })
    })?;
    Ok((graph, sccs))
}
