use syn::parse_quote;

use crate::ast::PostfixOp;
use crate::error::ConvertError;
use crate::expr::Expr;

fn codegen_format_err(detail: impl ToString) -> ConvertError {
    ConvertError::CodegenFormatError {
        detail: detail.to_string(),
    }
}

pub(crate) enum BodyLayout {
    /// `capture!(` continues on the same line as `let name =`.
    AssignmentContinuation,
    /// The whole `capture!(...)` block is indented starting at `base_column`.
    Block,
}

const WRAPPER_FN_BODY_INDENT: &str = "    ";

/// Rename `bind!` / `bind_slice!` to parseable function-call placeholders so `syn` /
/// `prettyplease` see the real argument length when breaking lines, then restore afterward.
const FMT_BIND: &str = "__pest_fmt_bind__";
const FMT_BIND_SLICE: &str = "__pest_fmt_bind_slice__";
const FMT_BIND_OPT: &str = "__pest_fmt_qmark__";
fn is_ident_start(ch: u8) -> bool {
    ch.is_ascii_alphabetic() || ch == b'_'
}

fn is_ident_continue(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_'
}

fn skip_rust_single_quote_literal(bytes: &[u8], index: &mut usize) {
    *index += 1;
    if *index >= bytes.len() {
        return;
    }
    if bytes[*index] == b'\\' {
        *index += 1;
        if *index < bytes.len() {
            *index += 1;
        }
        if *index < bytes.len() && bytes[*index] == b'\'' {
            *index += 1;
        }
        return;
    }
    if is_ident_start(bytes[*index]) {
        while *index < bytes.len() && is_ident_continue(bytes[*index]) {
            *index += 1;
        }
        // `'s'` char literal: ident immediately followed by `'`.
        // `'src` lifetime: ident followed by anything else.
        if *index < bytes.len() && bytes[*index] == b'\'' {
            *index += 1;
        }
        return;
    }
    // Any other single-character char literal, e.g. `'+'`.
    *index += 1;
    if *index < bytes.len() && bytes[*index] == b'\'' {
        *index += 1;
    }
}

pub(crate) fn peel_single_postfix<'a>(expr: &'a Expr) -> (&'a Expr, Option<&'a PostfixOp>) {
    match expr {
        Expr::Postfix { expr, op } => (expr.as_ref(), Some(op)),
        _ => (expr, None),
    }
}

fn append_bind_macro_body(source: &str, mut index: usize, result: &mut String) -> usize {
    let bytes = source.as_bytes();
    let mut depth = 1i32;
    let mut top_level_commas = 0u32;
    while index < bytes.len() && depth > 0 {
        let ch = bytes[index];
        if depth == 1 && ch == b',' {
            result.push(',');
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                result.push(bytes[index] as char);
                index += 1;
            }
            top_level_commas += 1;
            if top_level_commas == 1 && index < bytes.len() && bytes[index] == b'?' {
                result.push_str(FMT_BIND_OPT);
                index += 1;
                continue;
            }
            continue;
        }
        match ch {
            b'(' => {
                depth += 1;
                result.push('(');
                index += 1;
            }
            b')' => {
                depth -= 1;
                result.push(')');
                index += 1;
            }
            b'"' => {
                let start = index;
                index += 1;
                while index < bytes.len() && bytes[index] != b'"' {
                    if bytes[index] == b'\\' {
                        index += 1;
                    }
                    index += 1;
                }
                if index < bytes.len() {
                    index += 1;
                }
                result.push_str(&source[start..index]);
            }
            b'\'' => {
                let start = index;
                skip_rust_single_quote_literal(bytes, &mut index);
                result.push_str(&source[start..index]);
            }
            _ => {
                result.push(ch as char);
                index += 1;
            }
        }
    }
    index
}

fn substitute_bind_placeholders(source: &str) -> String {
    let mut result = String::new();
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if source[index..].starts_with("bind_slice!(") {
            result.push_str(FMT_BIND_SLICE);
            result.push('(');
            index += "bind_slice!(".len();
            index = append_bind_macro_body(source, index, &mut result);
        } else if source[index..].starts_with("bind!(") {
            result.push_str(FMT_BIND);
            result.push('(');
            index += "bind!(".len();
            index = append_bind_macro_body(source, index, &mut result);
        } else if bytes[index] == b'"' {
            result.push('"');
            index += 1;
            while index < bytes.len() && bytes[index] != b'"' {
                if bytes[index] == b'\\' {
                    result.push(bytes[index] as char);
                    index += 1;
                    if index < bytes.len() {
                        result.push(bytes[index] as char);
                        index += 1;
                    }
                    continue;
                }
                result.push(bytes[index] as char);
                index += 1;
            }
            if index < bytes.len() {
                result.push('"');
                index += 1;
            }
        } else if bytes[index] == b'\'' {
            let start = index;
            skip_rust_single_quote_literal(bytes, &mut index);
            result.push_str(&source[start..index]);
        } else {
            result.push(bytes[index] as char);
            index += 1;
        }
    }
    result
}

fn remove_trailing_commas_in_bind_macros(source: &str) -> String {
    let mut result = String::new();
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let bind_prefix = if source[index..].starts_with("bind_slice!(") {
            Some("bind_slice!(")
        } else if source[index..].starts_with("bind!(") {
            Some("bind!(")
        } else {
            None
        };
        if let Some(prefix) = bind_prefix {
            result.push_str(prefix);
            index += prefix.len();
            let mut depth = 1i32;
            while index < bytes.len() && depth > 0 {
                if depth == 1 && bytes[index] == b',' {
                    let mut next = index + 1;
                    while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                        next += 1;
                    }
                    if next < bytes.len() && bytes[next] == b')' {
                        index += 1;
                        continue;
                    }
                }
                let ch = bytes[index];
                match ch {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    b'"' => {
                        let start = index;
                        index += 1;
                        while index < bytes.len() && bytes[index] != b'"' {
                            if bytes[index] == b'\\' {
                                index += 1;
                            }
                            index += 1;
                        }
                        if index < bytes.len() {
                            index += 1;
                        }
                        result.push_str(&source[start..index]);
                        continue;
                    }
                    b'\'' => {
                        let start = index;
                        skip_rust_single_quote_literal(bytes, &mut index);
                        result.push_str(&source[start..index]);
                        continue;
                    }
                    _ => {}
                }
                result.push(ch as char);
                index += 1;
            }
        } else {
            result.push(bytes[index] as char);
            index += 1;
        }
    }
    result
}

fn restore_bind_placeholders(formatted: &str) -> String {
    let restored = formatted
        .replace(&format!("{FMT_BIND_SLICE}("), "bind_slice!(")
        .replace(&format!("{FMT_BIND}("), "bind!(")
        .replace(FMT_BIND_OPT, "?");
    remove_trailing_commas_in_bind_macros(&restored)
}

/// Pretty-print a Rust expression and indent it for embedding at `column` spaces.
pub(crate) fn format_expr_str(source: &str, column: usize) -> Result<String, ConvertError> {
    let source_for_parse = substitute_bind_placeholders(source);
    let expr: syn::Expr = syn::parse_str(&source_for_parse).map_err(codegen_format_err)?;
    let wrapper: syn::ItemFn = parse_quote! {
        fn __grammar_to_marser_fmt__() {
            #expr;
        }
    };
    let file = syn::File {
        shebang: None,
        attrs: vec![],
        items: vec![syn::Item::Fn(wrapper)],
    };
    let formatted = prettyplease::unparse(&file);
    let body = extract_fn_body_expr(&formatted)?;
    let normalized = dedent_lines(&strip_fn_body_indent(&body));
    let normalized = restore_bind_placeholders(&normalized);
    Ok(indent_lines(&normalized, column))
}

/// Format a `Parsed::...` construction for embedding inline after `capture!(... =>`.
pub(crate) fn format_construction_for_capture(source: &str, column: usize) -> Result<String, ConvertError> {
    let formatted = format_expr_str(source, column)?;
    Ok(match formatted.split_once('\n') {
        Some((first, rest)) if !rest.is_empty() => format!("{}\n{rest}", first.trim_start()),
        _ => formatted.trim_start().to_string(),
    })
}

fn strip_fn_body_indent(body: &str) -> String {
    body.lines()
        .map(|line| line.strip_prefix(WRAPPER_FN_BODY_INDENT).unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_fn_body_expr(formatted: &str) -> Result<String, ConvertError> {
    let file = syn::parse_file(formatted).map_err(codegen_format_err)?;
    let item_fn = match &file.items[0] {
        syn::Item::Fn(f) => f,
        _ => return Err(codegen_format_err("expected fn item")),
    };
    match item_fn.block.stmts.first() {
        Some(syn::Stmt::Expr(_, Some(_))) => {}
        _ => return Err(codegen_format_err("expected expr statement")),
    }

    let open = formatted
        .find('{')
        .ok_or_else(|| codegen_format_err("missing fn body"))?
        + 1;
    let close = formatted
        .rfind('}')
        .ok_or_else(|| codegen_format_err("missing fn body"))?;
    let body = formatted[open..close].trim();
    let body = body
        .strip_suffix(';')
        .ok_or_else(|| codegen_format_err("expected semicolon after expr"))?
        .trim();
    Ok(body.to_string())
}

fn dedent_lines(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let min_indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .into_iter()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else if line.len() >= min_indent {
                line[min_indent..].to_string()
            } else {
                line.trim_start().to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn indent_lines(text: &str, column: usize) -> String {
    let prefix = " ".repeat(column);
    dedent_lines(text)
        .lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const MAX_MATCHER_TUPLE_ARITY: usize = 12;

pub(crate) fn render_nested_tuple(parts: &[String]) -> String {
    if parts.is_empty() {
        return "()".to_string();
    }
    if parts.len() <= MAX_MATCHER_TUPLE_ARITY {
        return format!("({})", parts.join(", "));
    }
    let split = MAX_MATCHER_TUPLE_ARITY - 1;
    let mut outer = parts[..split].to_vec();
    outer.push(render_nested_tuple(&parts[split..]));
    format!("({})", outer.join(", "))
}

pub(crate) fn render_nested_one_of(parts: &[String]) -> String {
    if parts.is_empty() {
        return "one_of(())".to_string();
    }
    if parts.len() <= MAX_MATCHER_TUPLE_ARITY {
        return format!("one_of(({}))", parts.join(", "));
    }
    let split = MAX_MATCHER_TUPLE_ARITY - 1;
    let mut outer = parts[..split].to_vec();
    outer.push(render_nested_one_of(&parts[split..]));
    format!("one_of(({}))", outer.join(", "))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::ast::PostfixOp;
    use crate::output::BindSigil;

    #[test]
    fn dedent_strips_common_leading_spaces() {
        let input = "    ((\n        one_of('a'),\n        many('b'),\n    ))";
        assert_eq!(
            dedent_lines(input),
            "((\n    one_of('a'),\n    many('b'),\n))"
        );
    }

    #[test]
    fn indent_lines_applies_uniform_column() {
        let input = "((\n    one_of('a'),\n))";
        let out = indent_lines(input, 8);
        assert_eq!(out, "        ((\n            one_of('a'),\n        ))");
    }

    #[test]
    fn strip_fn_body_indent_removes_wrapper_padding() {
        let input = "    (\n        one_of('a'),\n    )";
        assert_eq!(strip_fn_body_indent(input), "(\n    one_of('a'),\n)");
    }

    #[test]
    fn format_expr_str_indents_nested_expression() {
        let source = "((one_of('a'), many('b')))";
        let out = format_expr_str(source, 8).unwrap();
        assert!(out.starts_with("        "));
        assert!(out.contains("one_of('a')"));
    }

    #[test]
    fn format_expr_str_breaks_long_nested_expression() {
        let source = "((
            one_of(('_', one_of(('a'..='z', 'A'..='Z')))),
            many(one_of(('_', one_of(('a'..='z', 'A'..='Z', '0'..='9'))))),
        ))";
        let out = format_expr_str(source, 8).unwrap();
        assert!(out.starts_with("        (("), "got:\n{out}");
        assert!(
            out.lines().any(|line| line.contains("one_of(('_',")),
            "got:\n{out}"
        );
        assert!(out.ends_with("        ))"), "got:\n{out}");
    }

    #[test]
    fn field_sigil_marks_repeated_rule_refs_as_multiple() {
        use crate::normalize::RuleDef;
        use crate::output::{FieldKey, analyze_rule_output, field_sigil_map};

        let expr = Expr::Sequence(vec![
            Expr::RuleRef("item".to_string()),
            Expr::Postfix {
                expr: Box::new(Expr::Sequence(vec![
                    Expr::Literal(",".to_string()),
                    Expr::RuleRef("item".to_string()),
                ])),
                op: PostfixOp::Repeat,
            },
        ]);
        let item = RuleDef {
            name: "item".to_string(),
            modifier: None,
            expr: Expr::Literal("x".to_string()),
            docs: Vec::new(),
        };
        let main = RuleDef {
            name: "main".to_string(),
            modifier: None,
            expr,
            docs: Vec::new(),
        };
        let rules: HashMap<String, &RuleDef> =
            [("item".to_string(), &item), ("main".to_string(), &main)]
                .into_iter()
                .collect();
        let spec = analyze_rule_output(&main.expr, &rules);
        let sigils = field_sigil_map(&spec);
        assert_eq!(
            sigils.get(&FieldKey::Rule("item".to_string())),
            Some(&BindSigil::Multiple)
        );
    }

    #[test]
    fn field_sigil_uses_optional_for_partial_one_of() {
        use crate::normalize::RuleDef;
        use crate::output::{FieldKey, analyze_rule_output, field_sigil_map};

        let expr = Expr::Choice(vec![
            Expr::Literal(" ".to_string()),
            Expr::RuleRef("newline".to_string()),
        ]);
        let newline = RuleDef {
            name: "newline".to_string(),
            modifier: None,
            expr: Expr::Literal("\n".to_string()),
            docs: Vec::new(),
        };
        let main = RuleDef {
            name: "main".to_string(),
            modifier: None,
            expr,
            docs: Vec::new(),
        };
        let rules: HashMap<String, &RuleDef> = [
            ("newline".to_string(), &newline),
            ("main".to_string(), &main),
        ]
        .into_iter()
        .collect();
        let spec = analyze_rule_output(&main.expr, &rules);
        let sigils = field_sigil_map(&spec);
        assert_eq!(
            sigils.get(&FieldKey::Rule("newline".to_string())),
            Some(&BindSigil::Optional)
        );
    }

    #[test]
    fn substitute_bind_placeholders_replaces_nested_parens() {
        let source = "(bind!(item.clone(), *item_val), repeat_ws((',', bind!(item.clone(), *item_val)), ws.clone()))";
        let substituted = substitute_bind_placeholders(source);
        assert!(substituted.contains("__pest_fmt_bind__(item.clone(), *item_val)"));
        assert!(!substituted.contains("bind!("));
        assert_eq!(restore_bind_placeholders(&substituted), source);
    }

    #[test]
    fn substitute_bind_placeholders_handles_ci_ch_char_literals() {
        let source = "bind_slice!((ci_ch('s'), ci_ch('e')), select as &'src str)";
        let substituted = substitute_bind_placeholders(source);
        assert!(
            substituted
                .contains("__pest_fmt_bind_slice__((ci_ch('s'), ci_ch('e')), select as &'src str)")
        );
        assert_eq!(restore_bind_placeholders(&substituted), source);
    }

    #[test]
    fn format_expr_str_case_insensitive_tagged_literals() {
        let source = "(start_of_input(), ws.clone(), bind_slice!((ci_ch('s'), ci_ch('e'), ci_ch('l'), ci_ch('e'), ci_ch('c'), ci_ch('t')), select as &'src str), ws.clone(), bind_slice!((ci_ch('f'), ci_ch('r'), ci_ch('o'), ci_ch('m')), from as &'src str), ws.clone(), bind!(ident.clone(), table), ws.clone(), end_of_input())";
        format_expr_str(source, 8).unwrap();
    }

    #[test]
    fn substitute_bind_placeholders_handles_bind_slice_with_lifetime() {
        let source = "repeat_ws((bind_slice!(one_of(('*', '/')), *op as &'src str), ws.clone()))";
        let substituted = substitute_bind_placeholders(source);
        assert!(
            substituted.contains("__pest_fmt_bind_slice__(one_of(('*', '/')), *op as &'src str)")
        );
        assert!(!substituted.contains("bind_slice!("));
        assert_eq!(restore_bind_placeholders(&substituted), source);
    }

    #[test]
    fn format_expr_str_pretty_prints_bind_expressions() {
        let source = "(start_of_input(), ws.clone(), bind!(item.clone(), *item_val), ws.clone(), repeat_ws((',', ws.clone(), bind!(item.clone(), *item_val)), ws.clone()), ws.clone(), end_of_input())";
        let out = format_expr_str(source, 8).unwrap();
        assert!(
            out.lines().count() > 1,
            "expected multiline output, got:\n{out}"
        );
        assert!(out.contains("bind!(item.clone(), *item_val)"));
    }

    #[test]
    fn restore_bind_placeholders_strips_trailing_commas() {
        let formatted = "__pest_fmt_bind_slice__((ci_ch('s'),), select as &'src str,)";
        let restored = restore_bind_placeholders(formatted);
        assert_eq!(restored, "bind_slice!((ci_ch('s'),), select as &'src str)");
    }

    #[test]
    fn substitute_bind_placeholders_handles_optional_sigil() {
        let source = r#"one_of((' ', '\t', bind!(newline.clone(), ?newline_val)))"#;
        let substituted = substitute_bind_placeholders(source);
        assert!(
            substituted
                .contains("__pest_fmt_bind__(newline.clone(), __pest_fmt_qmark__newline_val)")
        );
        assert_eq!(restore_bind_placeholders(&substituted), source);
        format_expr_str(source, 8).unwrap();
    }

    #[test]
    fn format_expr_str_breaks_repeat_ws_with_bind_slice() {
        let source = "(bind!(factor.clone(), *factor_val), ws.clone(), repeat_ws((bind_slice!(one_of(('*', '/')), *op as &'src str), ws.clone(), bind!(factor.clone(), *factor_val)), ws.clone()))";
        let out = format_expr_str(source, 8).unwrap();
        let repeat_ws_lines: Vec<_> = out
            .lines()
            .filter(|line| line.contains("bind_slice!") || line.contains("bind!(factor"))
            .collect();
        assert!(
            repeat_ws_lines.len() >= 2,
            "expected repeat_ws tuple elements on separate lines, got:\n{out}"
        );
    }

    #[test]
    fn format_expr_str_breaks_variant_construction() {
        let source =
            "Parsed::term { factor_val: factor_val.into_iter().map(Box::new).collect(), op: op }";
        let out = format_expr_str(source, 12).unwrap();
        assert!(
            out.lines().count() > 1,
            "expected multiline struct literal, got:\n{out}"
        );
        assert!(out.contains("factor_val:"));
    }

    #[test]
    fn render_nested_tuple_chunks_after_twelve_elements() {
        let parts: Vec<String> = (1..=13).map(|i| format!("p{i}")).collect();
        assert_eq!(
            render_nested_tuple(&parts),
            "(p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, (p12, p13))"
        );
    }

    #[test]
    fn render_nested_one_of_chunks_after_twelve_elements() {
        let parts: Vec<String> = (1..=13).map(|i| format!("p{i}")).collect();
        assert_eq!(
            render_nested_one_of(&parts),
            "one_of((p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, one_of((p12, p13))))"
        );
    }

    #[test]
    fn render_nested_tuple_chunks_long_insensitive_literal() {
        let parts: Vec<String> = "abcdefghijklm"
            .chars()
            .map(|c| format!("ci_ch({c:?})"))
            .collect();
        assert_eq!(
            render_nested_tuple(&parts),
            "(ci_ch('a'), ci_ch('b'), ci_ch('c'), ci_ch('d'), ci_ch('e'), ci_ch('f'), ci_ch('g'), ci_ch('h'), ci_ch('i'), ci_ch('j'), ci_ch('k'), (ci_ch('l'), ci_ch('m')))"
        );
    }
}
