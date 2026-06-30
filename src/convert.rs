use crate::ast::Grammar;
use crate::codegen::{CodegenOptions, generate_rust, prepare_codegen};
use crate::error::{
    ConvertError, ConvertResult, parse_error_from_furthest_fail, parse_error_from_parser_error,
};
use crate::grammar::parse_pest_grammar;
use crate::normalize::{RuleDef, RuleTable, build_rule_table};
use crate::peg::parse_peg_grammar;
use crate::syntax::InputSyntax;
use crate::validate::validate_all;
use marser::error::ParserError;
use marser::parser::Parser;

pub struct ConvertOptions {
    pub entry_rule: String,
    pub function_name: String,
    pub emit_comments: bool,
    pub emit_trace: bool,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            entry_rule: String::new(),
            function_name: "grammar".to_string(),
            emit_comments: true,
            emit_trace: false,
        }
    }
}

fn resolve_entry_rule(rules: &[RuleDef], entry_rule: &str) -> ConvertResult<String> {
    if !entry_rule.is_empty() {
        return Ok(entry_rule.to_string());
    }

    rules.last().map(|rule| rule.name.clone()).ok_or_else(|| {
        vec![ConvertError::UnknownEntryRule {
            name: "(no rules defined)".to_string(),
        }]
    })
}

fn parse_grammar_source(
    source: &str,
    syntax: InputSyntax,
) -> ConvertResult<(Grammar, Vec<ConvertError>)> {
    let (grammar, parse_errors) = match syntax {
        InputSyntax::Pest => parse_pest_grammar()
            .parse_str(source)
            .map_err(|err| vec![parse_error_from_furthest_fail(source, err)])?,
        InputSyntax::Peg => parse_peg_grammar()
            .parse_str(source)
            .map_err(|err| vec![parse_error_from_furthest_fail(source, err)])?,
    };

    let errors: Vec<ConvertError> = parse_errors
        .iter()
        .map(|e: &ParserError| parse_error_from_parser_error(source, e))
        .collect();
    if !errors.is_empty() {
        return Err(errors);
    }

    Ok((grammar, errors))
}

pub fn convert_source(
    source: &str,
    syntax: InputSyntax,
    options: &ConvertOptions,
) -> ConvertResult<String> {
    let (grammar, _) = parse_grammar_source(source, syntax)?;
    let table = build_rule_table(&grammar, syntax)?;
    convert_with_table(&table, options, Some(source))
}

pub fn convert_grammar_source(source: &str, options: &ConvertOptions) -> ConvertResult<String> {
    convert_source(source, InputSyntax::Pest, options)
}

fn convert_with_table(
    table: &RuleTable,
    options: &ConvertOptions,
    source: Option<&str>,
) -> ConvertResult<String> {
    let entry_rule = resolve_entry_rule(&table.rules, &options.entry_rule)?;
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
            function_name: options.function_name.clone(),
            source: source.map(str::to_string),
            emit_comments: options.emit_comments,
            emit_trace: options.emit_trace,
        },
    )
    .map_err(|e| vec![e])
}

pub fn list_rules(source: &str, syntax: InputSyntax) -> ConvertResult<Vec<String>> {
    let (grammar, _) = parse_grammar_source(source, syntax)?;
    let table = build_rule_table(&grammar, syntax)?;
    Ok(table.rules.iter().map(|r| r.name.clone()).collect())
}

pub fn list_grammar_rules(source: &str) -> ConvertResult<Vec<String>> {
    list_rules(source, InputSyntax::Pest)
}

pub fn convert_grammar(grammar: &Grammar, options: &ConvertOptions) -> ConvertResult<String> {
    let table = build_rule_table(grammar, InputSyntax::Pest)?;
    convert_with_table(&table, options, None)
}

pub fn convert_with_warnings(
    grammar: &Grammar,
    options: &ConvertOptions,
) -> ConvertResult<(String, Vec<String>)> {
    let table = build_rule_table(grammar, InputSyntax::Pest)?;
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
            source: None,
            emit_comments: options.emit_comments,
            emit_trace: options.emit_trace,
        },
    )
    .map_err(|e| vec![e])?;
    Ok((code, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::parse_pest_grammar;

    #[test]
    fn converts_simple_literal_rule() {
        let src = r#"main = { "hello" }"#;
        let grammar = parse_pest_grammar().parse_str(src).unwrap().0;
        let code = convert_grammar(
            &grammar,
            &ConvertOptions {
                entry_rule: "main".to_string(),
                function_name: "grammar".to_string(),
                ..Default::default()
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
        let grammar = parse_pest_grammar().parse_str(src).unwrap().0;
        let code = convert_grammar(
            &grammar,
            &ConvertOptions {
                entry_rule: String::new(),
                function_name: "grammar".to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(code.contains("\"world\""));
        assert!(!code.contains("\"hello\""));
    }

    #[test]
    fn emit_comments_false_omits_helper_comments() {
        let src = include_str!("../tests/fixtures/simple.pest");
        let code = convert_grammar_source(
            src,
            &ConvertOptions {
                entry_rule: "main".to_string(),
                emit_comments: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(code.contains("fn repeat_ws"));
        assert!(!code.contains("// Pest inserts implicit whitespace"));
    }

    #[test]
    fn emit_trace_false_omits_trace_markers() {
        let src = include_str!("../tests/fixtures/simple.pest");
        let code = convert_grammar_source(
            src,
            &ConvertOptions {
                entry_rule: "main".to_string(),
                emit_trace: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!code.contains(".trace()"));
        assert!(!code.contains("WithTrace"));
    }

    #[test]
    fn emit_trace_true_adds_reference_site_markers() {
        let src = include_str!("../tests/fixtures/simple.pest");
        let code = convert_grammar_source(
            src,
            &ConvertOptions {
                entry_rule: "main".to_string(),
                emit_trace: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(code.contains("use marser::trace::WithTrace;"));
        assert!(code.contains("bind!(item.clone(), *item_val).trace()"));
        assert!(code.contains("ws.clone().trace()"));
        assert!(code.contains("repeat_ws("));
        assert!(!code.contains(".erase_types().trace()"));
    }

    #[test]
    fn generates_parsed_enum_output() {
        let src = include_str!("../tests/fixtures/simple.pest");
        let code = convert_grammar_source(
            src,
            &ConvertOptions {
                entry_rule: "main".to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(code.contains("pub enum Parsed<'src>"));
        assert!(code.contains("Output = Parsed<'src>>"));
        assert!(code.contains("main {"));
        assert!(code.contains("item_val: Vec<Box<Parsed<'src>>>"));
        assert!(code.contains("ident { value: &'src str }"));
        assert!(code.contains("bind_slice!"));
    }

    #[test]
    fn trivia_rules_emit_matchers_not_parsers() {
        let src = include_str!("../tests/fixtures/simple.pest");
        let code = convert_grammar_source(
            src,
            &ConvertOptions {
                entry_rule: "main".to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        for variant in [
            "Parsed::WHITESPACE",
            "Parsed::COMMENT",
            "Parsed::newline",
            "Parsed::line_comment",
        ] {
            assert!(
                !code.contains(variant),
                "expected trivia variant {variant} to be omitted"
            );
        }
        assert!(code.contains("let newline = one_of(("));
        assert!(!code.contains("let newline = capture!("));
        assert!(code.contains("let WHITESPACE = one_of(("));
        assert!(code.contains("one_of((WHITESPACE.clone(), COMMENT.clone()))"));
        assert!(!code.contains("WHITESPACE.clone().ignore_result()"));
    }

    #[test]
    fn dual_use_silent_tab_is_matcher() {
        let src = include_str!("../tests/fixtures/dual_trivia.pest");
        let code = convert_grammar_source(
            src,
            &ConvertOptions {
                entry_rule: "main".to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!code.contains("Parsed::WHITESPACE"));
        assert!(!code.contains("Parsed::tab"));
        assert!(code.contains("let WHITESPACE = one_of(("));
        assert!(code.contains("let tab = "));
        assert!(!code.contains("let tab = capture!("));
        assert!(!code.contains("tab_val"));
    }

    #[test]
    fn silent_content_rule_emits_matcher_not_parser() {
        let src = include_str!("../tests/fixtures/ranges.pest");
        let code = convert_grammar_source(
            src,
            &ConvertOptions {
                entry_rule: "main".to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!code.contains("Parsed::hex_digit"));
        assert!(code.contains("let hex_digit = one_of("));
    }

    #[test]
    fn emit_trace_skips_silent_rule_references() {
        let src = include_str!("../tests/fixtures/simple.pest");
        let code = convert_grammar_source(
            src,
            &ConvertOptions {
                entry_rule: "main".to_string(),
                emit_trace: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!code.contains("bind!(newline.clone(), ?newline_val).trace()"));
    }

    #[test]
    fn converts_simple_peg_rule() {
        let src = r#"main <- "hello""#;
        let code = convert_source(
            src,
            InputSyntax::Peg,
            &ConvertOptions {
                entry_rule: "main".to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(code.contains("pub fn grammar"));
        assert!(code.contains("\"hello\""));
    }

    #[test]
    fn peg_source_comments_are_emitted() {
        let src = r#"main <- #v="hello""#;
        let code = convert_source(
            src,
            InputSyntax::Peg,
            &ConvertOptions {
                entry_rule: "main".to_string(),
                emit_comments: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(code.contains("// main <- #v=\"hello\""));
    }
}
