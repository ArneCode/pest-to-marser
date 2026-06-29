use crate::ast::Grammar;
use crate::codegen::{CodegenOptions, generate_rust, prepare_codegen};
use crate::error::{ConvertError, ConvertResult};
use crate::grammar::get_pest_grammar;
use crate::normalize::{RuleDef, RuleTable, build_rule_table};
use crate::validate::validate_all;
use marser::parser::Parser;

pub struct ConvertOptions {
    pub entry_rule: String,
    pub function_name: String,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            entry_rule: String::new(),
            function_name: "grammar".to_string(),
        }
    }
}

fn resolve_entry_rule(rules: &[RuleDef], entry_rule: &str) -> ConvertResult<String> {
    if !entry_rule.is_empty() {
        return Ok(entry_rule.to_string());
    }

    rules
        .last()
        .map(|rule| rule.name.clone())
        .ok_or_else(|| {
            vec![ConvertError::UnknownEntryRule {
                name: "(no rules defined)".to_string(),
            }]
        })
}

pub fn convert_pest_source(source: &str, options: &ConvertOptions) -> ConvertResult<String> {
    let (grammar, _errors) = get_pest_grammar().parse_str(source).map_err(|err| {
        vec![ConvertError::TrailingInput {
            remaining: err.to_string().len(),
        }]
    })?;

    convert_pest_grammar(&grammar, options)
}

fn convert_with_table(
    table: &RuleTable,
    entry_rule: &str,
    function_name: &str,
) -> ConvertResult<String> {
    let entry_rule = resolve_entry_rule(&table.rules, entry_rule)?;
    validate_all(&table.rules, &entry_rule)?;

    let (graph, sccs) = match prepare_codegen(table, &entry_rule) {
        Ok(v) => v,
        Err(err) => return Err(vec![err]),
    };

    generate_rust(
        table,
        &graph,
        &sccs,
        &CodegenOptions {
            function_name: function_name.to_string(),
        },
    )
    .map_err(|e| vec![e])
}

pub fn convert_pest_grammar(grammar: &Grammar, options: &ConvertOptions) -> ConvertResult<String> {
    let table = build_rule_table(grammar)?;
    convert_with_table(&table, &options.entry_rule, &options.function_name)
}

pub fn convert_with_warnings(
    grammar: &Grammar,
    options: &ConvertOptions,
) -> ConvertResult<(String, Vec<String>)> {
    let table = build_rule_table(grammar)?;
    let entry_rule = resolve_entry_rule(&table.rules, &options.entry_rule)?;
    validate_all(&table.rules, &entry_rule)?;
    let (graph, sccs) = match prepare_codegen(&table, &entry_rule) {
        Ok(v) => v,
        Err(err) => return Err(vec![err]),
    };
    let warnings = graph.warnings.clone();
    let code = generate_rust(
        &table,
        &graph,
        &sccs,
        &CodegenOptions {
            function_name: options.function_name.clone(),
        },
    )
    .map_err(|e| vec![e])?;
    Ok((code, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::get_pest_grammar;

    #[test]
    fn converts_simple_literal_rule() {
        let src = r#"main = { "hello" }"#;
        let grammar = get_pest_grammar().parse_str(src).unwrap().0;
        let code = convert_pest_grammar(
            &grammar,
            &ConvertOptions {
                entry_rule: "main".to_string(),
                function_name: "grammar".to_string(),
            },
        )
        .unwrap();
        assert!(code.contains("pub fn grammar"));
        assert!(code.contains("\"hello\""));
    }

    #[test]
    fn defaults_to_last_rule_when_entry_is_empty() {
        let src = r#"
WHITESPACE = _{ " " }
main = { "hello" }
other = { "world" }
"#;
        let grammar = get_pest_grammar().parse_str(src).unwrap().0;
        let code = convert_pest_grammar(
            &grammar,
            &ConvertOptions {
                entry_rule: String::new(),
                function_name: "grammar".to_string(),
            },
        )
        .unwrap();
        assert!(code.contains("\"world\""));
        assert!(!code.contains("\"hello\""));
    }
}
