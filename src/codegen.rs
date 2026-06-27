use std::collections::{HashMap, HashSet};

use syn::parse_quote;

use crate::ast::PostfixOp;
use crate::ast::PrefixOp;
use crate::error::ConvertError;
use crate::expr::{Builtin, Expr, MatchingContext, SymKey};
use crate::normalize::{RuleDef, RuleTable};
use crate::scc::{Scc, condensation_topo, is_cyclic, tarjan_scc};
use crate::specialize::{SpecializationGraph, build_specialization_graph, callee_context};

const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
    "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct",
    "super", "trait", "true", "type", "unsafe", "use", "where", "while", "yield",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodegenMode {
    Matcher,
}

pub struct CodegenOptions {
    pub function_name: String,
}

impl Default for CodegenOptions {
    fn default() -> Self {
        Self {
            function_name: "grammar".to_string(),
        }
    }
}

pub fn generate_rust(
    table: &RuleTable,
    graph: &SpecializationGraph,
    sccs: &[Scc],
    options: &CodegenOptions,
) -> Result<String, ConvertError> {
    let mut generator = Generator::new(table, graph, sccs, options);
    generator.emit()
}

fn codegen_format_err(detail: impl ToString) -> ConvertError {
    ConvertError::CodegenFormatError {
        detail: detail.to_string(),
    }
}

const WRAPPER_FN_BODY_INDENT: &str = "    ";

/// Pretty-print a Rust expression and indent it for embedding at `column` spaces.
fn format_expr_str(source: &str, column: usize) -> Result<String, ConvertError> {
    let expr: syn::Expr = syn::parse_str(source).map_err(codegen_format_err)?;
    let wrapper: syn::ItemFn = parse_quote! {
        fn __pest_to_marser_fmt__() {
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
    Ok(indent_lines(&normalized, column))
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

fn indent_lines(text: &str, column: usize) -> String {
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

fn indent_block(text: &str, column: usize) -> String {
    indent_lines(text, column)
}

struct Generator<'a> {
    table: &'a RuleTable,
    graph: &'a SpecializationGraph,
    sccs: &'a [Scc],
    options: &'a CodegenOptions,
    sym_names: HashMap<SymKey, String>,
    cyclic_syms: HashSet<SymKey>,
    extra_syms: HashSet<SymKey>,
    scc_map: HashMap<SymKey, usize>,
    referenced_builtins: HashSet<Builtin>,
    emitted: HashSet<SymKey>,
}

impl<'a> Generator<'a> {
    fn new(
        table: &'a RuleTable,
        graph: &'a SpecializationGraph,
        sccs: &'a [Scc],
        options: &'a CodegenOptions,
    ) -> Self {
        let contexts_by_rule = contexts_by_rule(&graph.nodes);
        let mut sym_names = HashMap::new();
        for sym in &graph.nodes {
            sym_names.insert(sym.clone(), binding_name_for_graph(sym, &contexts_by_rule));
        }

        let mut cyclic_syms = HashSet::new();
        let mut scc_map = HashMap::new();
        for (idx, scc) in sccs.iter().enumerate() {
            if is_cyclic(scc) {
                for member in &scc.members {
                    cyclic_syms.insert(member.clone());
                    scc_map.insert(member.clone(), idx);
                }
            }
        }

        let mut referenced_builtins = HashSet::new();
        for sym in &graph.nodes {
            if let Some(rule) = graph.rule_map.get(&sym.rule) {
                collect_builtins(&rule.expr, &mut referenced_builtins);
            }
        }

        let mut extra_syms = HashSet::new();
        if table.has_whitespace {
            let sym = SymKey {
                rule: "WHITESPACE".to_string(),
                context: MatchingContext::AtomicNoWs,
            };
            extra_syms.insert(sym);
        }
        if table.has_comment {
            let sym = SymKey {
                rule: "COMMENT".to_string(),
                context: MatchingContext::AtomicNoWs,
            };
            extra_syms.insert(sym);
        }

        Self {
            table,
            graph,
            sccs,
            options,
            sym_names,
            cyclic_syms,
            extra_syms,
            scc_map,
            referenced_builtins,
            emitted: HashSet::new(),
        }
    }

    fn emit(&mut self) -> Result<String, ConvertError> {
        let mut out = String::new();
        out.push_str("use marser::capture;\n");
        out.push_str("use marser::matcher::{\n");
        out.push_str("    AnyToken, MatcherCombinator, many, negative_lookahead, one_or_more,\n");
        out.push_str("    optional, positive_lookahead, start_of_input, end_of_input,\n");
        out.push_str("};\n");
        out.push_str("use marser::one_of::one_of;\n");
        out.push_str("use marser::parser::{\n");
        out.push_str("    DeferredWeak, Parser, ParserCombinator, recursive");
        let max_recursive = self
            .sccs
            .iter()
            .filter(|scc| is_cyclic(scc))
            .map(|scc| scc.members.len())
            .max()
            .unwrap_or(1);
        if max_recursive > 1 {
            for n in 2..=max_recursive.min(12) {
                out.push_str(&format!(", recursive{n}"));
            }
        }
        out.push_str("};\n\n");
        out.push_str(&format!(
            "pub fn {}<'src>() -> impl Parser<'src, &'src str, Output = ()> + Clone {{\n",
            sanitize_ident(&self.options.function_name)
        ));

        let order = condensation_topo(self.sccs, self.graph);
        let mut ws_emitted = !self.table.has_whitespace && !self.table.has_comment;

        for scc_idx in &order {
            let scc = &self.sccs[*scc_idx];
            let needs_ws = scc
                .members
                .iter()
                .any(|sym| sym.context == MatchingContext::NormalWs);
            if needs_ws && !ws_emitted {
                self.emit_ws_prerequisites(&mut out)?;
                self.emit_ws_matcher(&mut out)?;
                ws_emitted = true;
            }
            if is_cyclic(scc) {
                self.emit_recursive_scc(&mut out, scc)?;
            } else {
                for member in &scc.members {
                    self.emit_acyclic_sym(&mut out, member)?;
                }
            }
        }

        if !ws_emitted {
            self.emit_ws_matcher(&mut out)?;
        }

        let entry = &self.graph.entry;
        let entry_name = self.sym_names[entry].clone();
        out.push_str(&format!("    {entry_name}.clone()\n"));
        out.push_str("}\n");
        Ok(out)
    }

    fn emit_ws_prerequisites(&mut self, out: &mut String) -> Result<(), ConvertError> {
        let mut roots = Vec::new();
        if self.table.has_whitespace {
            roots.push(SymKey {
                rule: "WHITESPACE".to_string(),
                context: MatchingContext::AtomicNoWs,
            });
        }
        if self.table.has_comment {
            roots.push(SymKey {
                rule: "COMMENT".to_string(),
                context: MatchingContext::AtomicNoWs,
            });
        }

        let mut required = HashSet::new();
        let mut stack = roots;
        while let Some(sym) = stack.pop() {
            if !required.insert(sym.clone()) {
                continue;
            }
            if let Some(deps) = self.graph.edges.get(&sym) {
                for dep in deps {
                    stack.push(dep.clone());
                }
            }
        }

        let order = condensation_topo(self.sccs, self.graph);
        for scc_idx in &order {
            let scc = &self.sccs[*scc_idx];
            if !scc.members.iter().any(|member| required.contains(member)) {
                continue;
            }
            if is_cyclic(scc) {
                self.emit_recursive_scc(out, scc)?;
            } else {
                for member in &scc.members {
                    if required.contains(member) {
                        self.emit_acyclic_sym(out, member)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn emit_ws_matcher(&self, out: &mut String) -> Result<(), ConvertError> {
        if !self.table.has_whitespace && !self.table.has_comment {
            return Ok(());
        }

        let inner = match (self.table.has_whitespace, self.table.has_comment) {
            (true, true) => {
                let ws_name = self.sym_names[&SymKey {
                    rule: "WHITESPACE".to_string(),
                    context: MatchingContext::AtomicNoWs,
                }]
                .clone();
                let comment_name = self.sym_names[&SymKey {
                    rule: "COMMENT".to_string(),
                    context: MatchingContext::AtomicNoWs,
                }]
                .clone();
                format!(
                    "one_of(({ws_name}.clone().ignore_result(), {comment_name}.clone().ignore_result()))"
                )
            }
            (true, false) => {
                let ws_name = self.sym_names[&SymKey {
                    rule: "WHITESPACE".to_string(),
                    context: MatchingContext::AtomicNoWs,
                }]
                .clone();
                format!("{ws_name}.clone().ignore_result()")
            }
            (false, true) => {
                let comment_name = self.sym_names[&SymKey {
                    rule: "COMMENT".to_string(),
                    context: MatchingContext::AtomicNoWs,
                }]
                .clone();
                format!("{comment_name}.clone().ignore_result()")
            }
            (false, false) => unreachable!(),
        };
        let formatted = format_expr_str(&inner, 8)?;
        out.push_str(&format!("    let ws = many(\n{formatted}\n    );\n\n"));
        Ok(())
    }

    fn emit_acyclic_sym(&mut self, out: &mut String, sym: &SymKey) -> Result<(), ConvertError> {
        if self.emitted.contains(sym) || self.cyclic_syms.contains(sym) {
            return Ok(());
        }
        let name = self.sym_names[sym].clone();
        out.push_str(&format!("    let {name} = {};\n\n", self.gen_body(sym, None)?));
        self.emitted.insert(sym.clone());
        Ok(())
    }

    fn emit_recursive_scc(&mut self, out: &mut String, scc: &Scc) -> Result<(), ConvertError> {
        let n = scc.members.len();
        let names: Vec<String> = scc
            .members
            .iter()
            .map(|s| self.sym_names[s].clone())
            .collect();
        let params: Vec<String> = names.iter().map(|n| format!("{n}_weak")).collect();

        if n == 1 {
            let sym = &scc.members[0];
            let name = &names[0];
            let body = indent_block(&self.gen_body(sym, Some(&scc.members))?, 8);
            out.push_str(&format!(
                "    let {name} = recursive(|{name}_weak|\n{body}\n    );\n\n"
            ));
            self.emitted.insert(sym.clone());
            return Ok(());
        }

        out.push_str(&format!(
            "    let ({}) = recursive{n}(|{}| (\n",
            names.join(", "),
            params.join(", ")
        ));
        for (idx, sym) in scc.members.iter().enumerate() {
            let body = indent_block(&self.gen_body(sym, Some(&scc.members))?, 8);
            let sep = if idx + 1 == scc.members.len() {
                ""
            } else {
                ","
            };
            out.push_str(&format!("{body}{sep}\n"));
            self.emitted.insert(sym.clone());
        }
        out.push_str("    ));\n\n");
        Ok(())
    }

    fn gen_body(
        &self,
        sym: &SymKey,
        recursive_members: Option<&[SymKey]>,
    ) -> Result<String, ConvertError> {
        let rule = &self.graph.rule_map[&sym.rule];
        let inner = self.gen_expr(
            &rule.expr,
            sym.context,
            recursive_members,
            CodegenMode::Matcher,
        );
        let formatted_grammar = format_expr_str(&inner, 8)?;
        Ok(format!(
            "capture!(\n{formatted_grammar} => ()\n    ).erase_types()"
        ))
    }

    fn gen_expr(
        &self,
        expr: &Expr,
        ctx: MatchingContext,
        recursive_members: Option<&[SymKey]>,
        mode: CodegenMode,
    ) -> String {
        match (expr, mode) {
            (Expr::Empty, _) => "()".to_string(),
            (Expr::Literal(s), _) => self.gen_string_matcher(s),
            (Expr::InsensitiveLiteral(s), _) => self.gen_insensitive_matcher(s),
            (Expr::Builtin(b), _) => self.gen_builtin_matcher(*b),
            (Expr::Range { start, end }, _) => format!("{start:?}..={end:?}"),
            (Expr::RuleRef(name), mode) => self.gen_rule_ref(name, ctx, recursive_members, mode),
            (Expr::Sequence(items), mode) => self.gen_sequence(items, ctx, recursive_members, mode),
            (Expr::Choice(items), mode) => {
                let parts: Vec<_> = items
                    .iter()
                    .map(|item| self.gen_expr(item, ctx, recursive_members, mode))
                    .collect();
                format!("one_of(({}))", parts.join(", "))
            }
            (Expr::Prefix { op, expr }, _) => {
                let inner = self.gen_expr(expr, ctx, recursive_members, CodegenMode::Matcher);
                match op {
                    PrefixOp::PositivePredicate => format!("positive_lookahead({inner})"),
                    PrefixOp::NegativePredicate => format!("negative_lookahead({inner})"),
                }
            }
            (Expr::Postfix { expr, op }, _) => self.gen_postfix(expr, op, ctx, recursive_members),
        }
    }

    fn uses_ws(&self, ctx: MatchingContext) -> bool {
        (self.table.has_whitespace || self.table.has_comment) && ctx == MatchingContext::NormalWs
    }

    fn gen_sequence(
        &self,
        items: &[Expr],
        ctx: MatchingContext,
        recursive_members: Option<&[SymKey]>,
        mode: CodegenMode,
    ) -> String {
        let has_ws = self.table.has_whitespace || self.table.has_comment;
        let mut parts = Vec::new();
        for (idx, item) in items.iter().enumerate() {
            if idx > 0 && has_ws && ctx == MatchingContext::NormalWs {
                parts.push("ws.clone()".to_string());
            }
            parts.push(self.gen_expr(item, ctx, recursive_members, mode));
        }
        if mode == CodegenMode::Matcher {
            format!("({})", parts.join(", "))
        } else {
            unreachable!("sequences are emitted as matcher tuples")
        }
    }

    fn gen_postfix(
        &self,
        expr: &Expr,
        op: &PostfixOp,
        ctx: MatchingContext,
        recursive_members: Option<&[SymKey]>,
    ) -> String {
        let inner = self.gen_expr(expr, ctx, recursive_members, CodegenMode::Matcher);
        match op {
            PostfixOp::Optional => format!("optional({inner})"),
            PostfixOp::Repeat => self.gen_unbounded_repeat(&inner, ctx, false),
            PostfixOp::RepeatOnce => self.gen_unbounded_repeat(&inner, ctx, true),
            PostfixOp::RepeatExact(n) => self.gen_repeat_count(&inner, ctx, *n, *n),
            PostfixOp::RepeatMin(n) if *n == 0 => self.gen_unbounded_repeat(&inner, ctx, false),
            PostfixOp::RepeatMin(n) => self.gen_repeat_min(&inner, ctx, *n, None),
            PostfixOp::RepeatMax(n) => self.gen_repeat_max(&inner, ctx, *n),
            PostfixOp::RepeatMinMax(min, max) if *min == 0 => {
                self.gen_repeat_max(&inner, ctx, *max)
            }
            PostfixOp::RepeatMinMax(min, max) => self.gen_repeat_min(&inner, ctx, *min, Some(*max)),
        }
    }

    fn gen_unbounded_repeat(
        &self,
        inner: &str,
        ctx: MatchingContext,
        at_least_one: bool,
    ) -> String {
        if self.uses_ws(ctx) {
            if !at_least_one {
                return format!("optional(({inner}, many((ws.clone(), {inner}))))");
            }
            return format!("({inner}, many((ws.clone(), {inner})))");
        }
        if !at_least_one {
            return format!("many({inner})");
        }
        format!("one_or_more({inner})")
    }

    fn gen_repeat_count(&self, inner: &str, ctx: MatchingContext, min: u32, max: u32) -> String {
        let mut parts = self.separated_items(inner, ctx, min);
        if max > min {
            parts.push(self.gen_optional_repeated(inner, ctx, max - min));
        }
        if parts.len() == 1 {
            parts.pop().unwrap()
        } else {
            format!("({})", parts.join(", "))
        }
    }

    fn gen_repeat_min(
        &self,
        inner: &str,
        ctx: MatchingContext,
        min: u32,
        max: Option<u32>,
    ) -> String {
        let mut parts = self.separated_items(inner, ctx, min);
        let tail = match max {
            Some(max) if max > min => self.gen_optional_repeated(inner, ctx, max - min),
            None if self.uses_ws(ctx) => format!("many((ws.clone(), {inner}))"),
            None => format!("many({inner})"),
            _ => return format!("({})", parts.join(", ")),
        };
        parts.push(tail);
        format!("({})", parts.join(", "))
    }

    fn gen_repeat_max(&self, inner: &str, ctx: MatchingContext, max: u32) -> String {
        if max == 0 {
            return "()".to_string();
        }
        self.gen_optional_repeated(inner, ctx, max)
    }

    fn gen_optional_repeated(&self, inner: &str, ctx: MatchingContext, max: u32) -> String {
        if max == 0 {
            return "()".to_string();
        }
        if max == 1 {
            return format!("optional({inner})");
        }
        if self.uses_ws(ctx) {
            return format!(
                "optional(({inner}, optional((ws.clone(), {}))))",
                self.gen_optional_repeated(inner, ctx, max - 1)
            );
        }
        format!(
            "optional(({inner}, {}))",
            self.gen_optional_repeated(inner, ctx, max - 1)
        )
    }

    fn separated_items(&self, inner: &str, ctx: MatchingContext, count: u32) -> Vec<String> {
        let mut parts = Vec::new();
        for i in 0..count {
            if i > 0 && self.uses_ws(ctx) {
                parts.push("ws.clone()".to_string());
            }
            parts.push(inner.to_string());
        }
        parts
    }

    fn gen_rule_ref(
        &self,
        name: &str,
        ctx: MatchingContext,
        recursive_members: Option<&[SymKey]>,
        mode: CodegenMode,
    ) -> String {
        if let Some(rule) = self.graph.rule_map.get(name) {
            let callee_ctx = callee_context(ctx, rule.modifier.as_ref());
            let sym = SymKey {
                rule: name.to_string(),
                context: callee_ctx,
            };
            let reference = self.sym_ref(&sym, recursive_members);
            return format!("{reference}.ignore_result()");
        }
        if let Some(builtin) = Builtin::from_name(name) {
            return self.gen_builtin_matcher(builtin);
        }
        "()".to_string()
    }

    fn sym_ref(&self, sym: &SymKey, recursive_members: Option<&[SymKey]>) -> String {
        let name = self.sym_names.get(sym).unwrap_or_else(|| {
            panic!("missing specialized symbol: {}:{:?}", sym.rule, sym.context)
        });
        if let Some(members) = recursive_members {
            if members.iter().any(|m| m == sym) {
                return format!("{name}_weak.clone()");
            }
        }
        format!("{name}.clone()")
    }

    fn gen_string_matcher(&self, s: &str) -> String {
        if s.is_empty() {
            return "()".to_string();
        }
        if s.chars().count() == 1 {
            let ch = s.chars().next().unwrap();
            format!("{ch:?}")
        } else {
            format!("{s:?}")
        }
    }

    fn gen_insensitive_matcher(&self, s: &str) -> String {
        if s.is_empty() {
            return "()".to_string();
        }
        let parts: Vec<String> = s
            .chars()
            .map(|c| {
                if c.is_ascii_alphabetic() {
                    let lower = c.to_ascii_lowercase();
                    let upper = c.to_ascii_uppercase();
                    if lower == upper {
                        format!("{c:?}")
                    } else {
                        format!("one_of(({lower:?}, {upper:?}))")
                    }
                } else {
                    format!("{c:?}")
                }
            })
            .collect();
        format!("({})", parts.join(", "))
    }

    fn gen_builtin_matcher(&self, b: Builtin) -> String {
        match b {
            Builtin::Soi => "start_of_input()".to_string(),
            Builtin::Eoi => "end_of_input()".to_string(),
            Builtin::Any => "AnyToken".to_string(),
            Builtin::Newline => "one_of((\"\\n\", \"\\r\\n\"))".to_string(),
            Builtin::AsciiDigit => "'0'..='9'".to_string(),
            Builtin::AsciiNonzeroDigit => "'1'..='9'".to_string(),
            Builtin::AsciiBinDigit => "one_of(('0', '1'))".to_string(),
            Builtin::AsciiOctDigit => "'0'..='7'".to_string(),
            Builtin::AsciiHexDigit => "one_of(('0'..='9', 'a'..='f', 'A'..='F'))".to_string(),
            Builtin::AsciiAlphaLower => "'a'..='z'".to_string(),
            Builtin::AsciiAlphaUpper => "'A'..='Z'".to_string(),
            Builtin::AsciiAlpha => "one_of(('a'..='z', 'A'..='Z'))".to_string(),
            Builtin::AsciiAlphanumeric => "one_of(('a'..='z', 'A'..='Z', '0'..='9'))".to_string(),
        }
    }

    fn gen_string_literal(&self, s: &str) -> String {
        if s.is_empty() {
            return "capture!(() => ())".to_string();
        }
        if s.chars().count() == 1 {
            let ch = s.chars().next().unwrap();
            return format!("capture!({ch:?} => ())");
        }
        format!("capture!({s:?} => ())")
    }

    fn gen_insensitive(&self, s: &str) -> String {
        if s.is_empty() {
            return "capture!(() => ())".to_string();
        }
        let parts: Vec<String> = s
            .chars()
            .map(|c| {
                if c.is_ascii_alphabetic() {
                    let lower = c.to_ascii_lowercase();
                    let upper = c.to_ascii_uppercase();
                    if lower == upper {
                        format!("{c:?}")
                    } else {
                        format!("one_of(({lower:?}, {upper:?}))")
                    }
                } else {
                    format!("{c:?}")
                }
            })
            .collect();
        format!("capture!(({}) => ())", parts.join(", "))
    }

    fn gen_builtin(&self, b: Builtin) -> String {
        match b {
            Builtin::Soi => "start_of_input()".to_string(),
            Builtin::Eoi => "end_of_input()".to_string(),
            Builtin::Any => "AnyToken".to_string(),
            Builtin::Newline => "one_of((\"\\n\", \"\\r\\n\"))".to_string(),
            Builtin::AsciiDigit => "capture!('0'..='9' => ())".to_string(),
            Builtin::AsciiNonzeroDigit => "capture!('1'..='9' => ())".to_string(),
            Builtin::AsciiBinDigit => "one_of(('0', '1'))".to_string(),
            Builtin::AsciiOctDigit => "capture!('0'..='7' => ())".to_string(),
            Builtin::AsciiHexDigit => "one_of(('0'..='9', 'a'..='f', 'A'..='F'))".to_string(),
            Builtin::AsciiAlphaLower => "capture!('a'..='z' => ())".to_string(),
            Builtin::AsciiAlphaUpper => "capture!('A'..='Z' => ())".to_string(),
            Builtin::AsciiAlpha => "one_of(('a'..='z', 'A'..='Z'))".to_string(),
            Builtin::AsciiAlphanumeric => "one_of(('a'..='z', 'A'..='Z', '0'..='9'))".to_string(),
        }
    }
}

fn collect_sym_deps(
    expr: &Expr,
    caller_context: MatchingContext,
    rules: &HashMap<String, RuleDef>,
    sym_names: &mut HashMap<SymKey, String>,
) {
    match expr {
        Expr::RuleRef(name) => {
            if let Some(rule) = rules.get(name) {
                let context = callee_context(caller_context, rule.modifier.as_ref());
                let sym = SymKey {
                    rule: name.clone(),
                    context,
                };
                if sym_names.contains_key(&sym) {
                    return;
                }
                sym_names.insert(sym.clone(), binding_name(&sym));
                collect_sym_deps(&rule.expr, context, rules, sym_names);
            }
        }
        Expr::Sequence(items) | Expr::Choice(items) => {
            for item in items {
                collect_sym_deps(item, caller_context, rules, sym_names);
            }
        }
        Expr::Prefix { expr, .. } | Expr::Postfix { expr, .. } => {
            collect_sym_deps(expr, caller_context, rules, sym_names);
        }
        _ => {}
    }
}

fn collect_builtins(expr: &Expr, out: &mut HashSet<Builtin>) {
    match expr {
        Expr::Builtin(b) => {
            out.insert(*b);
        }
        Expr::Sequence(items) | Expr::Choice(items) => {
            for item in items {
                collect_builtins(item, out);
            }
        }
        Expr::Prefix { expr, .. } | Expr::Postfix { expr, .. } => collect_builtins(expr, out),
        Expr::RuleRef(name) => {
            if let Some(b) = Builtin::from_name(name) {
                out.insert(b);
            }
        }
        _ => {}
    }
}

pub fn binding_name(sym: &SymKey) -> String {
    let base = sanitize_ident(&sym.rule);
    let suffix = match sym.context {
        MatchingContext::NormalWs => "__nw",
        MatchingContext::AtomicNoWs => "__anw",
    };
    format!("{base}{suffix}")
}

pub fn sanitize_ident(name: &str) -> String {
    if RUST_KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

fn contexts_by_rule(nodes: &HashSet<SymKey>) -> HashMap<String, HashSet<MatchingContext>> {
    let mut contexts = HashMap::new();
    for sym in nodes {
        contexts
            .entry(sym.rule.clone())
            .or_insert_with(HashSet::new)
            .insert(sym.context);
    }
    contexts
}

fn binding_name_for_graph(
    sym: &SymKey,
    contexts_by_rule: &HashMap<String, HashSet<MatchingContext>>,
) -> String {
    let base = sanitize_ident(&sym.rule);
    if contexts_by_rule
        .get(&sym.rule)
        .is_some_and(|contexts| contexts.len() > 1)
    {
        let suffix = match sym.context {
            MatchingContext::NormalWs => "__nw",
            MatchingContext::AtomicNoWs => "__anw",
        };
        format!("{base}{suffix}")
    } else {
        base
    }
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

#[cfg(test)]
mod format_tests {
    use super::*;

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
}
