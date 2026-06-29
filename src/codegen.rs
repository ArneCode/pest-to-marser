use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};

use syn::parse_quote;

use crate::ast::Modifier;
use crate::ast::PostfixOp;
use crate::ast::PrefixOp;
use crate::error::ConvertError;
use crate::expr::{Builtin, Expr, MatchingContext, SymKey};
use crate::normalize::{RuleDef, RuleTable};
use crate::scc::{
    Scc, condensation_topo, is_cyclic, partition_scc_for_recursion, recursive_arity, tarjan_scc,
};
use crate::specialize::{SpecializationGraph, build_specialization_graph, callee_context, collect_rule_deps};

const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
    "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct",
    "super", "trait", "true", "type", "unsafe", "use", "where", "while", "yield",
];

static AGENT_DEBUG_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);

fn agent_debug_log(hypothesis_id: &str, location: &str, message: &str, data: String) {
    if AGENT_DEBUG_LOG_COUNT.fetch_add(1, Ordering::Relaxed) >= 10 {
        return;
    }
    // #region agent log
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/home/arne/projects/parsing/pest-to-marser/.cursor/debug-51eef6.log")
    {
        let _ = writeln!(
            file,
            "{{\"sessionId\":\"51eef6\",\"runId\":\"initial\",\"hypothesisId\":{:?},\"location\":{:?},\"message\":{:?},\"data\":{},\"timestamp\":{}}}",
            hypothesis_id,
            location,
            message,
            data,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
    }
    // #endregion
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodegenMode {
    Matcher,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BindSigil {
    Plain,
    Optional,
    Multiple,
}

impl BindSigil {
    fn prefix(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Optional => "?",
            Self::Multiple => "*",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OccurrenceClass {
    Plain,
    Optional,
    Multiple,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WrappingPostfix {
    Optional,
    Repeat,
}

fn occurrence_class(wrapping: Option<WrappingPostfix>) -> OccurrenceClass {
    match wrapping {
        Some(WrappingPostfix::Optional) => OccurrenceClass::Optional,
        Some(WrappingPostfix::Repeat) => OccurrenceClass::Multiple,
        None => OccurrenceClass::Plain,
    }
}

fn collect_bind_occurrences(
    expr: &Expr,
    in_lookahead: bool,
    wrapping: Option<WrappingPostfix>,
    out: &mut HashMap<String, Vec<OccurrenceClass>>,
) {
    match expr {
        Expr::RuleRef(name) => {
            if !in_lookahead {
                out.entry(name.clone())
                    .or_default()
                    .push(occurrence_class(wrapping));
            }
        }
        Expr::Prefix { op, expr } => {
            let in_la = matches!(
                op,
                PrefixOp::PositivePredicate | PrefixOp::NegativePredicate
            );
            collect_bind_occurrences(expr, in_lookahead || in_la, wrapping, out);
        }
        Expr::Postfix { expr, op } => {
            let inner_wrapping = match op {
                PostfixOp::Optional => Some(WrappingPostfix::Optional),
                PostfixOp::Repeat
                | PostfixOp::RepeatOnce
                | PostfixOp::RepeatExact(_)
                | PostfixOp::RepeatMin(_)
                | PostfixOp::RepeatMax(_)
                | PostfixOp::RepeatMinMax(_, _) => Some(WrappingPostfix::Repeat),
            };
            collect_bind_occurrences(expr, in_lookahead, inner_wrapping, out);
        }
        Expr::Sequence(items) => {
            for item in items {
                collect_bind_occurrences(item, in_lookahead, wrapping, out);
            }
        }
        Expr::Choice(items) => {
            let per_alt: Vec<HashMap<String, Vec<OccurrenceClass>>> = items
                .iter()
                .map(|item| {
                    let mut alt_map = HashMap::new();
                    collect_bind_occurrences(item, in_lookahead, wrapping, &mut alt_map);
                    alt_map
                })
                .collect();
            let num_alts = items.len();
            let mut all_names = HashSet::new();
            for alt_map in &per_alt {
                all_names.extend(alt_map.keys().cloned());
            }
            for name in all_names {
                let alts_containing = per_alt.iter().filter(|m| m.contains_key(&name)).count();
                if alts_containing < num_alts {
                    out.entry(name).or_default().push(OccurrenceClass::Optional);
                } else {
                    for alt_map in &per_alt {
                        if let Some(classes) = alt_map.get(&name) {
                            out.entry(name.clone()).or_default().extend(classes.iter().copied());
                        }
                    }
                }
            }
        }
        Expr::Empty
        | Expr::Builtin(_)
        | Expr::Literal(_)
        | Expr::InsensitiveLiteral(_)
        | Expr::Range { .. } => {}
    }
}

fn dominant_sigil_from_occurrences(classes: &[OccurrenceClass]) -> BindSigil {
    if classes.is_empty() {
        return BindSigil::Plain;
    }
    if classes
        .iter()
        .any(|class| matches!(class, OccurrenceClass::Multiple))
    {
        return BindSigil::Multiple;
    }
    if classes.len() > 1 {
        return BindSigil::Multiple;
    }
    match classes[0] {
        OccurrenceClass::Plain => BindSigil::Plain,
        OccurrenceClass::Optional => BindSigil::Optional,
        OccurrenceClass::Multiple => BindSigil::Multiple,
    }
}

fn dominant_sigil_map(expr: &Expr) -> HashMap<String, BindSigil> {
    let mut occurrences: HashMap<String, Vec<OccurrenceClass>> = HashMap::new();
    collect_bind_occurrences(expr, false, None, &mut occurrences);
    occurrences
        .into_iter()
        .map(|(name, classes)| (name, dominant_sigil_from_occurrences(&classes)))
        .collect()
}

fn net_brace_delta(line: &str) -> i32 {
    let mut depth = 0i32;
    for ch in line.chars() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    depth
}

fn is_rule_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn parse_rule_start(line: &str) -> Option<(String, i32)> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") {
        return None;
    }
    let eq_pos = trimmed.find('=')?;
    let name = trimmed[..eq_pos].trim();
    if name.is_empty() || !name.chars().all(is_rule_name_char) {
        return None;
    }
    let rest = &trimmed[eq_pos + 1..];
    Some((name.to_string(), net_brace_delta(rest)))
}

pub fn extract_rule_source_comments(source: &str) -> HashMap<String, String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut map = HashMap::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if let Some((name, mut brace_depth)) = parse_rule_start(line) {
            let mut rule_lines = vec![line.to_string()];
            index += 1;
            while index < lines.len() && brace_depth > 0 {
                let next = lines[index];
                rule_lines.push(next.to_string());
                brace_depth += net_brace_delta(next);
                index += 1;
            }
            let comment = rule_lines
                .iter()
                .map(|rule_line| format!("// {}", rule_line.trim_end()))
                .collect::<Vec<_>>()
                .join("\n");
            map.insert(name, comment);
        } else {
            index += 1;
        }
    }
    map
}

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
    let mut generator = Generator::new(table, graph, sccs, options);
    generator.emit()
}

fn codegen_format_err(detail: impl ToString) -> ConvertError {
    ConvertError::CodegenFormatError {
        detail: detail.to_string(),
    }
}

enum BodyLayout {
    /// `capture!(` continues on the same line as `let name =`.
    AssignmentContinuation,
    /// The whole `capture!(...)` block is indented starting at `base_column`.
    Block,
}

const WRAPPER_FN_BODY_INDENT: &str = "    ";

/// Replace each `bind!(...)` with a parseable placeholder so `syn` / `prettyplease` can format
/// the surrounding expression, then restore the original `bind!` sites afterward.
fn substitute_bind_placeholders(source: &str) -> (String, Vec<String>) {
    let mut result = String::new();
    let mut originals = Vec::new();
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if source[index..].starts_with("bind!(") {
            let start = index;
            index += "bind!(".len();
            let mut depth = 1i32;
            while index < bytes.len() && depth > 0 {
                match bytes[index] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    b'"' => {
                        index += 1;
                        while index < bytes.len() && bytes[index] != b'"' {
                            if bytes[index] == b'\\' {
                                index += 1;
                            }
                            index += 1;
                        }
                    }
                    b'\'' => {
                        index += 1;
                        while index < bytes.len() && bytes[index] != b'\'' {
                            if bytes[index] == b'\\' {
                                index += 1;
                            }
                            index += 1;
                        }
                    }
                    _ => {}
                }
                index += 1;
            }
            originals.push(source[start..index].to_string());
            let placeholder = format!("__pest_fmt_bind_{}__", originals.len() - 1);
            result.push_str(&placeholder);
        } else {
            result.push(bytes[index] as char);
            index += 1;
        }
    }
    (result, originals)
}

fn restore_bind_placeholders(formatted: &str, originals: &[String]) -> String {
    let mut result = formatted.to_string();
    for (index, original) in originals.iter().enumerate() {
        let placeholder = format!("__pest_fmt_bind_{index}__");
        result = result.replace(&placeholder, original);
    }
    result
}

/// Pretty-print a Rust expression and indent it for embedding at `column` spaces.
fn format_expr_str(source: &str, column: usize) -> Result<String, ConvertError> {
    let (source_for_parse, bind_originals) = substitute_bind_placeholders(source);
    let expr: syn::Expr = syn::parse_str(&source_for_parse).map_err(codegen_format_err)?;
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
    let normalized = restore_bind_placeholders(&normalized, &bind_originals);
    if normalized.lines().count() > source.lines().count() || source.len() > 80 {
        agent_debug_log(
            "H4",
            "src/codegen.rs:format_expr_str",
            "pretty formatter expanded expression layout",
            format!(
                "{{\"sourceChars\":{},\"sourceLines\":{},\"outputLines\":{},\"column\":{}}}",
                source.len(),
                source.lines().count(),
                normalized.lines().count(),
                column
            ),
        );
    }
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ImportNeeds {
    any_token: bool,
    matcher: bool,
    many: bool,
    negative_lookahead: bool,
    one_or_more: bool,
    optional: bool,
    positive_lookahead: bool,
    repeat: bool,
    start_of_input: bool,
    end_of_input: bool,
    one_of: bool,
    recursive: bool,
}

fn builtin_uses_one_of(b: Builtin) -> bool {
    matches!(
        b,
        Builtin::Newline
            | Builtin::AsciiBinDigit
            | Builtin::AsciiHexDigit
            | Builtin::AsciiAlpha
            | Builtin::AsciiAlphanumeric
    )
}

fn collect_builtin_import_needs(b: Builtin, out: &mut ImportNeeds) {
    match b {
        Builtin::Any => out.any_token = true,
        Builtin::Soi => out.start_of_input = true,
        Builtin::Eoi => out.end_of_input = true,
        b if builtin_uses_one_of(b) => out.one_of = true,
        _ => {}
    }
}

fn uses_ws_context(table: &RuleTable, ctx: MatchingContext) -> bool {
    (table.has_whitespace || table.has_comment) && ctx == MatchingContext::NormalWs
}

fn collect_bounded_repeat_import_needs(min: u32, max: Option<u32>, uses_ws: bool, out: &mut ImportNeeds) {
    if uses_ws {
        if min == 0 {
            let Some(max) = max else {
                return;
            };
            if max == 0 {
                return;
            }
            if max == 1 {
                out.optional = true;
                return;
            }
            out.optional = true;
            out.repeat = true;
            return;
        }
        out.repeat = true;
        return;
    }
    out.repeat = true;
}

fn collect_postfix_import_needs(
    inner: &Expr,
    op: &PostfixOp,
    ctx: MatchingContext,
    table: &RuleTable,
    out: &mut ImportNeeds,
    in_lookahead: bool,
) {
    collect_import_needs_expr(inner, ctx, table, out, in_lookahead);
    let uses_ws = uses_ws_context(table, ctx);
    match op {
        PostfixOp::Optional => out.optional = true,
        PostfixOp::Repeat | PostfixOp::RepeatMin(0) => {
            if !uses_ws {
                out.many = true;
            }
        }
        PostfixOp::RepeatOnce => {
            if !uses_ws {
                out.one_or_more = true;
            }
        }
        PostfixOp::RepeatExact(n) => collect_bounded_repeat_import_needs(0, Some(*n), uses_ws, out),
        PostfixOp::RepeatMin(n) => collect_bounded_repeat_import_needs(*n, None, uses_ws, out),
        PostfixOp::RepeatMax(n) => collect_bounded_repeat_import_needs(0, Some(*n), uses_ws, out),
        PostfixOp::RepeatMinMax(min, max) if *min == 0 => {
            collect_bounded_repeat_import_needs(0, Some(*max), uses_ws, out);
        }
        PostfixOp::RepeatMinMax(min, max) => {
            collect_bounded_repeat_import_needs(*min, Some(*max), uses_ws, out);
        }
    }
}

fn collect_import_needs_expr(
    expr: &Expr,
    ctx: MatchingContext,
    table: &RuleTable,
    out: &mut ImportNeeds,
    in_lookahead: bool,
) {
    match expr {
        Expr::Empty => {}
        Expr::Builtin(b) => collect_builtin_import_needs(*b, out),
        Expr::Literal(_) | Expr::Range { .. } => {}
        Expr::InsensitiveLiteral(_) => {}
        Expr::RuleRef(name) => {
            if let Some(builtin) = Builtin::from_name(name) {
                collect_builtin_import_needs(builtin, out);
            }
        }
        Expr::Sequence(items) | Expr::Choice(items) => {
            if matches!(expr, Expr::Choice(_)) {
                out.one_of = true;
            }
            for item in items {
                collect_import_needs_expr(item, ctx, table, out, in_lookahead);
            }
        }
        Expr::Prefix { op, expr } => {
            let in_la = matches!(
                op,
                PrefixOp::PositivePredicate | PrefixOp::NegativePredicate
            );
            match op {
                PrefixOp::PositivePredicate => out.positive_lookahead = true,
                PrefixOp::NegativePredicate => out.negative_lookahead = true,
            }
            collect_import_needs_expr(expr, ctx, table, out, in_lookahead || in_la);
        }
        Expr::Postfix { expr, op } => {
            collect_postfix_import_needs(expr, op, ctx, table, out, in_lookahead);
        }
    }
}

fn collect_ws_rule_import_needs(
    table: &RuleTable,
    graph: &SpecializationGraph,
    needs: &mut ImportNeeds,
) {
    let needs_implicit_ws = graph
        .nodes
        .iter()
        .any(|sym| sym.context == MatchingContext::NormalWs);
    if !needs_implicit_ws {
        return;
    }

    let mut stack = Vec::new();
    if table.has_whitespace {
        stack.push(SymKey {
            rule: "WHITESPACE".to_string(),
            context: MatchingContext::AtomicNoWs,
        });
    }
    if table.has_comment {
        stack.push(SymKey {
            rule: "COMMENT".to_string(),
            context: MatchingContext::AtomicNoWs,
        });
    }

    let mut visited = HashSet::new();
    while let Some(sym) = stack.pop() {
        if !visited.insert(sym.clone()) {
            continue;
        }
        let Some(rule) = graph.rule_map.get(&sym.rule) else {
            continue;
        };
        collect_import_needs_expr(&rule.expr, sym.context, table, needs, false);
        let mut deps = HashSet::new();
        collect_rule_deps(&rule.expr, sym.context, &graph.rule_map, &mut deps);
        for dep in deps {
            stack.push(dep);
        }
    }

    if table.has_whitespace || table.has_comment {
        needs.matcher = true;
        needs.many = true;
        if table.has_whitespace && table.has_comment {
            needs.one_of = true;
        }
    }
}

fn compute_import_needs(
    table: &RuleTable,
    graph: &SpecializationGraph,
    cyclic_syms: &HashSet<SymKey>,
    referenced_builtins: &HashSet<Builtin>,
    needs_ws_repeat_helper: bool,
    needs_ws_repeat_once_helper: bool,
    needs_ci_ch_helper: bool,
    needs_bounded_repeat: bool,
) -> ImportNeeds {
    let mut needs = ImportNeeds::default();
    for sym in &graph.nodes {
        if let Some(rule) = graph.rule_map.get(&sym.rule) {
            collect_import_needs_expr(&rule.expr, sym.context, table, &mut needs, false);
        }
    }
    collect_ws_rule_import_needs(table, graph, &mut needs);
    if needs_ws_repeat_helper {
        needs.matcher = true;
        needs.optional = true;
        needs.many = true;
    }
    if needs_ws_repeat_once_helper {
        needs.matcher = true;
        needs.many = true;
    }
    if needs_ci_ch_helper {
        needs.matcher = true;
        needs.one_of = true;
    }
    if needs_bounded_repeat {
        needs.repeat = true;
    }
    if !cyclic_syms.is_empty() {
        needs.recursive = true;
    }
    for builtin in referenced_builtins {
        collect_builtin_import_needs(*builtin, &mut needs);
    }
    needs
}

fn push_braced_use_list(out: &mut String, path: &str, items: &[&str]) {
    out.push_str("use ");
    out.push_str(path);
    out.push_str("{\n");
    for item in items {
        out.push_str("    ");
        out.push_str(item);
        out.push_str(",\n");
    }
    out.push_str("};\n");
}

struct Generator<'a> {
    table: &'a RuleTable,
    graph: &'a SpecializationGraph,
    sccs: &'a [Scc],
    options: &'a CodegenOptions,
    rule_comments: HashMap<String, String>,
    sym_names: HashMap<SymKey, String>,
    cyclic_syms: HashSet<SymKey>,
    extra_syms: HashSet<SymKey>,
    scc_map: HashMap<SymKey, usize>,
    referenced_builtins: HashSet<Builtin>,
    emitted: HashSet<SymKey>,
    needs_ws_repeat_helper: bool,
    needs_ws_repeat_once_helper: bool,
    needs_ci_ch_helper: bool,
    needs_bounded_repeat: bool,
    import_needs: ImportNeeds,
    recursive_comment_emitted: bool,
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

        let needs_implicit_ws = graph
            .nodes
            .iter()
            .any(|sym| sym.context == MatchingContext::NormalWs);
        if needs_implicit_ws {
            if table.has_whitespace {
                let sym = SymKey {
                    rule: "WHITESPACE".to_string(),
                    context: MatchingContext::AtomicNoWs,
                };
                sym_names
                    .entry(sym)
                    .or_insert_with(|| "WHITESPACE".to_string());
            }
            if table.has_comment {
                let sym = SymKey {
                    rule: "COMMENT".to_string(),
                    context: MatchingContext::AtomicNoWs,
                };
                sym_names
                    .entry(sym)
                    .or_insert_with(|| "COMMENT".to_string());
            }
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

        let has_ws = table.has_whitespace || table.has_comment;
        let needs_ws_repeat_helper = has_ws
            && graph.nodes.iter().any(|sym| {
                sym.context == MatchingContext::NormalWs
                    && graph
                        .rule_map
                        .get(&sym.rule)
                        .is_some_and(|rule| expr_needs_ws_repeat_helper(&rule.expr))
            });
        let needs_ws_repeat_once_helper = has_ws
            && graph.nodes.iter().any(|sym| {
                sym.context == MatchingContext::NormalWs
                    && graph
                        .rule_map
                        .get(&sym.rule)
                        .is_some_and(|rule| expr_needs_ws_repeat_once_helper(&rule.expr))
            });
        let needs_ci_ch_helper = graph.nodes.iter().any(|sym| {
            graph
                .rule_map
                .get(&sym.rule)
                .is_some_and(|rule| expr_has_insensitive_literal(&rule.expr))
        });
        let needs_bounded_repeat = graph.nodes.iter().any(|sym| {
            graph
                .rule_map
                .get(&sym.rule)
                .is_some_and(|rule| expr_needs_bounded_repeat(&rule.expr))
        });

        let rule_comments = options
            .source
            .as_deref()
            .map(extract_rule_source_comments)
            .unwrap_or_default();

        let import_needs = compute_import_needs(
            table,
            graph,
            &cyclic_syms,
            &referenced_builtins,
            needs_ws_repeat_helper,
            needs_ws_repeat_once_helper,
            needs_ci_ch_helper,
            needs_bounded_repeat,
        );

        Self {
            table,
            graph,
            sccs,
            options,
            rule_comments,
            sym_names,
            cyclic_syms,
            extra_syms,
            scc_map,
            referenced_builtins,
            emitted: HashSet::new(),
            needs_ws_repeat_helper,
            needs_ws_repeat_once_helper,
            needs_ci_ch_helper,
            needs_bounded_repeat,
            import_needs,
            recursive_comment_emitted: false,
        }
    }

    fn emit_recursive_explanation(&mut self, out: &mut String) {
        if !self.options.emit_comments || self.recursive_comment_emitted {
            return;
        }
        self.recursive_comment_emitted = true;
        out.push_str(
            "    // This rule cluster is cyclic: some rules refer back to others in the same\n\
             \x20   // group (directly or indirectly). marser's `recursive` breaks that cycle\n\
             \x20   // by giving the closure a deferred handle to clone inside the body. See:\n\
             \x20   // https://docs.rs/marser/latest/marser/parser/deferred/fn.recursive.html\n",
        );
    }

    fn emit_rule_comment(&self, out: &mut String, rule_name: &str, indent: &str) {
        if !self.options.emit_comments {
            return;
        }
        if let Some(comment) = self.rule_comments.get(rule_name) {
            for line in comment.lines() {
                out.push_str(indent);
                out.push_str(line);
                out.push('\n');
            }
        }
    }

    fn emit_imports(&self, out: &mut String) {
        out.push_str("use marser::capture;\n");

        let mut matcher = Vec::new();
        if self.import_needs.any_token {
            matcher.push("AnyToken");
        }
        if self.import_needs.matcher {
            matcher.push("Matcher");
        }
        if self.import_needs.many {
            matcher.push("many");
        }
        if self.import_needs.negative_lookahead {
            matcher.push("negative_lookahead");
        }
        if self.import_needs.one_or_more {
            matcher.push("one_or_more");
        }
        if self.import_needs.repeat {
            matcher.push("repeat");
        }
        if self.import_needs.optional {
            matcher.push("optional");
        }
        if self.import_needs.positive_lookahead {
            matcher.push("positive_lookahead");
        }
        if self.import_needs.start_of_input {
            matcher.push("start_of_input");
        }
        if self.import_needs.end_of_input {
            matcher.push("end_of_input");
        }
        if !matcher.is_empty() {
            push_braced_use_list(out, "marser::matcher::", &matcher);
        }
        if self.import_needs.one_of {
            out.push_str("use marser::one_of::one_of;\n");
        }

        let max_recursive = self
            .sccs
            .iter()
            .filter(|scc| is_cyclic(scc))
            .map(|scc| recursive_arity(scc, self.graph))
            .max()
            .unwrap_or(1);
        let mut parser = vec!["Parser".to_string(), "ParserCombinator".to_string()];
        if self.import_needs.recursive {
            parser.push("recursive".to_string());
            for n in 2..=max_recursive.min(12) {
                parser.push(format!("recursive{n}"));
            }
        }
        out.push_str("use marser::parser::{\n");
        for item in &parser {
            out.push_str("    ");
            out.push_str(item);
            out.push_str(",\n");
        }
        out.push_str("};\n");
        if self.options.emit_trace {
            out.push_str("use marser::trace::WithTrace;\n");
        }
        out.push('\n');
    }

    fn emit(&mut self) -> Result<String, ConvertError> {
        let mut out = String::new();
        self.emit_imports(&mut out);
        if self.needs_ws_repeat_helper {
            if self.options.emit_comments {
                out.push_str(
                    "// Pest inserts implicit whitespace between repetitions, but not before the\n\
                     // first item. This keeps `X*` equivalent to Pest while avoiding duplicated\n\
                     // generated matcher bodies.\n",
                );
            }
            out.push_str(
                "fn repeat_ws<'src, MRes, Item, Ws>(\n\
                 \x20   item: Item,\n\
                 \x20   ws: Ws,\n\
                 ) -> impl Matcher<'src, &'src str, MRes> + Clone\n\
                 where\n\
                 \x20   Item: Matcher<'src, &'src str, MRes> + Clone,\n\
                 \x20   Ws: Matcher<'src, &'src str, MRes> + Clone,\n\
                 {\n\
                 \x20   optional((item.clone(), many((ws, item))))\n\
                 }\n\n",
            );
        }
        if self.needs_ws_repeat_once_helper {
            if self.options.emit_comments {
                out.push_str(
                    "// Pest `X+` requires a first item, then implicit whitespace only between\n\
                     // later repetitions. This helper preserves that shape without duplicating\n\
                     // the generated matcher body for `X`.\n",
                );
            }
            out.push_str(
                "fn repeat_one_or_more_ws<'src, MRes, Item, Ws>(\n\
                 \x20   item: Item,\n\
                 \x20   ws: Ws,\n\
                 ) -> impl Matcher<'src, &'src str, MRes> + Clone\n\
                 where\n\
                 \x20   Item: Matcher<'src, &'src str, MRes> + Clone,\n\
                 \x20   Ws: Matcher<'src, &'src str, MRes> + Clone,\n\
                 {\n\
                 \x20   (item.clone(), many((ws, item)))\n\
                 }\n\n",
            );
        }
        if self.needs_ci_ch_helper {
            if self.options.emit_comments {
                out.push_str(
                    "// Pest `^\"...\"` literals match ASCII letters case-insensitively.\n",
                );
            }
            out.push_str(
                "fn ci_ch<'src, MRes>(c: char) -> impl Matcher<'src, &'src str, MRes> + Clone {\n\
                 \x20   one_of((c.to_ascii_lowercase(), c.to_ascii_uppercase()))\n\
                 }\n\n",
            );
        }
        out.push_str(&format!(
            "pub fn {}<'src>() -> impl Parser<'src, &'src str, Output = ()> + Clone {{\n",
            sanitize_ident(&self.options.function_name)
        ));

        self.emit_hoisted_builtins(&mut out)?;

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

        if !ws_emitted && self.graph.nodes.iter().any(|sym| sym.context == MatchingContext::NormalWs) {
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

        let mut visiting = HashSet::new();
        for root in roots {
            self.emit_ws_sym(out, &root, &mut visiting)?;
        }
        Ok(())
    }

    fn emit_ws_sym(
        &mut self,
        out: &mut String,
        sym: &SymKey,
        visiting: &mut HashSet<SymKey>,
    ) -> Result<(), ConvertError> {
        if self.emitted.contains(sym) {
            return Ok(());
        }
        if !visiting.insert(sym.clone()) {
            return Ok(());
        }

        if let Some(deps) = self.graph.edges.get(sym) {
            for dep in deps {
                self.emit_ws_sym(out, dep, visiting)?;
            }
        } else if let Some(rule) = self.graph.rule_map.get(&sym.rule) {
            let mut deps = HashSet::new();
            collect_rule_deps(&rule.expr, sym.context, &self.graph.rule_map, &mut deps);
            for dep in deps {
                self.emit_ws_sym(out, &dep, visiting)?;
            }
        }

        self.emit_acyclic_sym(out, sym)?;
        visiting.remove(sym);
        Ok(())
    }

    fn emit_hoisted_builtins(&self, out: &mut String) -> Result<(), ConvertError> {
        if self.referenced_builtins.is_empty() {
            return Ok(());
        }
        let mut builtins: Vec<Builtin> = self
            .referenced_builtins
            .iter()
            .copied()
            .filter(|b| should_hoist_builtin(*b))
            .collect();
        if builtins.is_empty() {
            return Ok(());
        }
        builtins.sort_by_key(|b| b.name());
        for builtin in builtins {
            let name = sanitize_ident(builtin.name());
            let expr = builtin_matcher_expr(builtin);
            out.push_str(&format!("    let {name} = {expr};\n\n"));
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
        if !self.sym_names.contains_key(sym) {
            let contexts_by_rule = contexts_by_rule(&self.graph.nodes);
            self.sym_names.insert(
                sym.clone(),
                binding_name_for_graph(sym, &contexts_by_rule),
            );
        }
        let name = self.sym_names[sym].clone();
        self.emit_rule_comment(out, &sym.rule, "    ");
        out.push_str(&format!(
            "    let {name} = {};\n\n",
            self.gen_body(sym, None, 4, BodyLayout::AssignmentContinuation)?
        ));
        self.emitted.insert(sym.clone());
        Ok(())
    }

    fn emit_recursive_scc(&mut self, out: &mut String, scc: &Scc) -> Result<(), ConvertError> {
        if scc.members.iter().all(|m| self.emitted.contains(m)) {
            return Ok(());
        }

        let (fvs, non_fvs_topo) = partition_scc_for_recursion(scc, self.graph);
        let fvs_refs = &fvs;

        let mut locals = String::new();
        for sym in &non_fvs_topo {
            let name = self.sym_names[sym].clone();
            self.emit_rule_comment(&mut locals, &sym.rule, "        ");
            let body = self.gen_body(sym, Some(fvs_refs), 8, BodyLayout::AssignmentContinuation)?;
            locals.push_str(&format!("        let {name} = {body};\n\n"));
        }

        let fvs_names: Vec<String> = fvs.iter().map(|s| self.sym_names[s].clone()).collect();
        let n = fvs.len();

        self.emit_recursive_explanation(out);

        if n == 1 {
            let fvs_sym = &fvs[0];
            let fvs_name = &fvs_names[0];
            let fvs_body = self.gen_body(fvs_sym, Some(fvs_refs), 8, BodyLayout::Block)?;
            if non_fvs_topo.is_empty() {
                self.emit_rule_comment(out, &fvs_sym.rule, "    ");
                out.push_str(&format!(
                    "    let {fvs_name} = recursive(|{fvs_name}|\n{fvs_body}\n    );\n\n"
                ));
            } else {
                let mut fvs_comment = String::new();
                self.emit_rule_comment(&mut fvs_comment, &fvs_sym.rule, "        ");
                out.push_str(&format!(
                    "    let {fvs_name} = recursive(|{fvs_name}| {{\n{locals}{fvs_comment}{fvs_body}\n    }});\n\n"
                ));
            }
        } else {
            out.push_str(&format!(
                "    let ({}) = recursive{n}(|{}| {{\n",
                fvs_names.join(", "),
                fvs_names.join(", ")
            ));
            out.push_str(&locals);
            out.push_str("        (\n");
            for (idx, sym) in fvs.iter().enumerate() {
                self.emit_rule_comment(out, &sym.rule, "        ");
                let body = self.gen_body(sym, Some(fvs_refs), 12, BodyLayout::Block)?;
                let sep = if idx + 1 == fvs.len() { "" } else { "," };
                out.push_str(&format!("{body}{sep}\n"));
            }
            out.push_str("        )\n    });\n\n");
        }

        for member in &scc.members {
            self.emitted.insert(member.clone());
        }
        Ok(())
    }

    fn gen_body(
        &self,
        sym: &SymKey,
        recursive_members: Option<&[SymKey]>,
        base_column: usize,
        layout: BodyLayout,
    ) -> Result<String, ConvertError> {
        let rule = &self.graph.rule_map[&sym.rule];
        let sigil_map = dominant_sigil_map(&rule.expr);
        let inner = self.gen_expr(
            &rule.expr,
            sym.context,
            recursive_members,
            CodegenMode::Matcher,
            &sigil_map,
            false,
        );
        agent_debug_log(
            "H1,H5",
            "src/codegen.rs:gen_body",
            "rule normalized expression and emitted matcher body",
            format!(
                "{{\"rule\":{:?},\"context\":{:?},\"expr\":{:?},\"body\":{:?}}}",
                sym.rule, sym.context, rule.expr, inner
            ),
        );
        let inner_column = match layout {
            BodyLayout::AssignmentContinuation => base_column + 4,
            BodyLayout::Block => 4,
        };
        let formatted_grammar = format_expr_str(&inner, inner_column)?;
        let close_indent = match layout {
            BodyLayout::AssignmentContinuation => " ".repeat(base_column),
            BodyLayout::Block => String::new(),
        };
        let capture = format!(
            "capture!(\n{formatted_grammar} => ()\n{close_indent}).erase_types()"
        );
        Ok(match layout {
            BodyLayout::AssignmentContinuation => capture,
            BodyLayout::Block => indent_lines(&capture, base_column),
        })
    }

    fn gen_expr(
        &self,
        expr: &Expr,
        ctx: MatchingContext,
        recursive_members: Option<&[SymKey]>,
        mode: CodegenMode,
        sigil_map: &HashMap<String, BindSigil>,
        in_lookahead: bool,
    ) -> String {
        match (expr, mode) {
            (Expr::Empty, _) => "()".to_string(),
            (Expr::Literal(s), _) => self.gen_string_matcher(s),
            (Expr::InsensitiveLiteral(s), _) => self.gen_insensitive_matcher(s),
            (Expr::Builtin(b), _) => self.gen_builtin_matcher(*b),
            (Expr::Range { start, end }, _) => format!("{start:?}..={end:?}"),
            (Expr::RuleRef(name), mode) => {
                self.gen_rule_ref(name, ctx, recursive_members, mode, sigil_map, in_lookahead)
            }
            (Expr::Sequence(items), mode) => {
                self.gen_sequence(items, ctx, recursive_members, mode, sigil_map, in_lookahead)
            }
            (Expr::Choice(items), mode) => {
                let parts: Vec<_> = items
                    .iter()
                    .map(|item| {
                        self.gen_expr(
                            item,
                            ctx,
                            recursive_members,
                            mode,
                            sigil_map,
                            in_lookahead,
                        )
                    })
                    .collect();
                format!("one_of(({}))", parts.join(", "))
            }
            (Expr::Prefix { op, expr }, _) => {
                let in_la = matches!(
                    op,
                    PrefixOp::PositivePredicate | PrefixOp::NegativePredicate
                );
                let inner = self.gen_expr(
                    expr,
                    ctx,
                    recursive_members,
                    CodegenMode::Matcher,
                    sigil_map,
                    in_lookahead || in_la,
                );
                match op {
                    PrefixOp::PositivePredicate => format!("positive_lookahead({inner})"),
                    PrefixOp::NegativePredicate => format!("negative_lookahead({inner})"),
                }
            }
            (Expr::Postfix { expr, op }, _) => {
                self.gen_postfix(
                    expr,
                    op,
                    ctx,
                    recursive_members,
                    sigil_map,
                    in_lookahead,
                )
            }
        }
    }

    fn uses_ws(&self, ctx: MatchingContext) -> bool {
        (self.table.has_whitespace || self.table.has_comment) && ctx == MatchingContext::NormalWs
    }

    fn ws_ref(&self) -> String {
        if self.options.emit_trace {
            "ws.clone().trace()".to_string()
        } else {
            "ws.clone()".to_string()
        }
    }

    fn should_trace_rule(&self, rule: &RuleDef) -> bool {
        self.options.emit_trace && rule.modifier != Some(Modifier::Silent)
    }

    fn trace_bind(
        &self,
        reference: &str,
        sigil_prefix: &str,
        bind_name: &str,
        rule: &RuleDef,
    ) -> String {
        let bind = format!("bind!({reference}, {sigil_prefix}{bind_name})");
        if self.should_trace_rule(rule) {
            format!("{bind}.trace()")
        } else {
            bind
        }
    }

    fn gen_sequence(
        &self,
        items: &[Expr],
        ctx: MatchingContext,
        recursive_members: Option<&[SymKey]>,
        mode: CodegenMode,
        sigil_map: &HashMap<String, BindSigil>,
        in_lookahead: bool,
    ) -> String {
        let has_ws = self.table.has_whitespace || self.table.has_comment;
        let mut parts = Vec::new();
        for (idx, item) in items.iter().enumerate() {
            if idx > 0 && has_ws && ctx == MatchingContext::NormalWs {
                parts.push(self.ws_ref());
            }
            parts.push(self.gen_expr(
                item,
                ctx,
                recursive_members,
                mode,
                sigil_map,
                in_lookahead,
            ));
        }
        if has_ws && ctx == MatchingContext::NormalWs && items.len() > 1 {
            agent_debug_log(
                "H2",
                "src/codegen.rs:gen_sequence",
                "normal-context sequence inserted whitespace matchers",
                format!(
                    "{{\"itemCount\":{},\"insertedWhitespace\":{},\"parts\":{:?}}}",
                    items.len(),
                    items.len().saturating_sub(1),
                    parts
                ),
            );
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
        sigil_map: &HashMap<String, BindSigil>,
        in_lookahead: bool,
    ) -> String {
        let inner = self.gen_expr(
            expr,
            ctx,
            recursive_members,
            CodegenMode::Matcher,
            sigil_map,
            in_lookahead,
        );
        match op {
            PostfixOp::Optional => format!("optional({inner})"),
            PostfixOp::Repeat => self.gen_unbounded_repeat(&inner, ctx, false),
            PostfixOp::RepeatOnce => self.gen_unbounded_repeat(&inner, ctx, true),
            PostfixOp::RepeatExact(n) => self.gen_bounded_repeat(&inner, ctx, *n, Some(*n)),
            PostfixOp::RepeatMin(n) if *n == 0 => self.gen_unbounded_repeat(&inner, ctx, false),
            PostfixOp::RepeatMin(n) => self.gen_bounded_repeat(&inner, ctx, *n, None),
            PostfixOp::RepeatMax(n) => self.gen_bounded_repeat(&inner, ctx, 0, Some(*n)),
            PostfixOp::RepeatMinMax(min, max) if *min == 0 => {
                self.gen_bounded_repeat(&inner, ctx, 0, Some(*max))
            }
            PostfixOp::RepeatMinMax(min, max) => {
                self.gen_bounded_repeat(&inner, ctx, *min, Some(*max))
            }
        }
    }

    fn gen_unbounded_repeat(
        &self,
        inner: &str,
        ctx: MatchingContext,
        at_least_one: bool,
    ) -> String {
        if self.uses_ws(ctx) {
            let ws = self.ws_ref();
            if !at_least_one {
                let rendered = format!("repeat_ws({inner}, {ws})");
                agent_debug_log(
                    "H3",
                    "src/codegen.rs:gen_unbounded_repeat",
                    "whitespace-aware zero-or-more lowered through helper",
                    format!(
                        "{{\"atLeastOne\":{},\"usesWhitespace\":true,\"inner\":{:?},\"rendered\":{:?}}}",
                        at_least_one, inner, rendered
                    ),
                );
                return rendered;
            }
            let rendered = format!("repeat_one_or_more_ws({inner}, {ws})");
            agent_debug_log(
                "H3",
                "src/codegen.rs:gen_unbounded_repeat",
                "whitespace-aware one-or-more lowered through helper",
                format!(
                    "{{\"atLeastOne\":{},\"usesWhitespace\":true,\"inner\":{:?},\"rendered\":{:?}}}",
                    at_least_one, inner, rendered
                ),
            );
            return rendered;
        }
        if !at_least_one {
            return format!("many({inner})");
        }
        format!("one_or_more({inner})")
    }

    fn gen_bounded_repeat(
        &self,
        inner: &str,
        ctx: MatchingContext,
        min: u32,
        max: Option<u32>,
    ) -> String {
        let min = min as usize;
        let max = max.map(|value| value as usize);

        if self.uses_ws(ctx) {
            let ws = self.ws_ref();
            if min == 0 {
                let Some(max) = max else {
                    return self.gen_unbounded_repeat(inner, ctx, false);
                };
                if max == 0 {
                    return "()".to_string();
                }
                if max == 1 {
                    return format!("optional({inner})");
                }
                return format!(
                    "optional(({inner}, repeat(({ws}, {inner}), 0..={})))",
                    max - 1
                );
            }

            let ws_repeat = format!("({ws}, {inner})");
            return match max {
                Some(max) if max == min => {
                    if min == 1 {
                        inner.to_string()
                    } else {
                        format!("({inner}, repeat({ws_repeat}, {}..={}))", min - 1, max - 1)
                    }
                }
                Some(max) => format!("({inner}, repeat({ws_repeat}, {}..={}))", min - 1, max - 1),
                None => format!("({inner}, repeat({ws_repeat}, {}..))", min - 1),
            };
        }

        match max {
            Some(max) => format!("repeat({inner}, {min}..={max})"),
            None => format!("repeat({inner}, {min}..)"),
        }
    }

    fn gen_rule_ref(
        &self,
        name: &str,
        ctx: MatchingContext,
        recursive_members: Option<&[SymKey]>,
        mode: CodegenMode,
        sigil_map: &HashMap<String, BindSigil>,
        in_lookahead: bool,
    ) -> String {
        if let Some(rule) = self.graph.rule_map.get(name) {
            let callee_ctx = callee_context(ctx, rule.modifier.as_ref());
            let sym = SymKey {
                rule: name.to_string(),
                context: callee_ctx,
            };
            let reference = self.sym_ref(&sym, recursive_members);
            if in_lookahead {
                return format!("{reference}.ignore_result()");
            }
            let sigil = sigil_map.get(name).copied().unwrap_or(BindSigil::Plain);
            let bind_name = bind_var_name(name);
            return self.trace_bind(&reference, sigil.prefix(), &bind_name, rule);
        }
        if let Some(builtin) = Builtin::from_name(name) {
            return self.gen_builtin_matcher(builtin);
        }
        let _ = mode;
        "()".to_string()
    }

    fn sym_ref(&self, sym: &SymKey, recursive_members: Option<&[SymKey]>) -> String {
        let name = self.sym_names.get(sym).unwrap_or_else(|| {
            panic!("missing specialized symbol: {}:{:?}", sym.rule, sym.context)
        });
        if let Some(members) = recursive_members {
            if members.iter().any(|m| m == sym) {
                return format!("{name}.clone()");
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
        let parts: Vec<String> = s.chars().map(|c| format!("ci_ch({c:?})")).collect();
        if parts.len() == 1 {
            parts.into_iter().next().unwrap()
        } else {
            format!("({})", parts.join(", "))
        }
    }

    fn gen_builtin_matcher(&self, b: Builtin) -> String {
        if should_hoist_builtin(b) && self.referenced_builtins.contains(&b) {
            return format!("{}.clone()", sanitize_ident(b.name()));
        }
        builtin_matcher_expr(b)
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
        let parts: Vec<String> = s.chars().map(|c| format!("ci_ch({c:?})")).collect();
        format!("capture!(({}) => ())", parts.join(", "))
    }

    fn gen_builtin(&self, b: Builtin) -> String {
        if should_hoist_builtin(b) && self.referenced_builtins.contains(&b) {
            return format!(
                "capture!({}.clone() => ())",
                sanitize_ident(b.name())
            );
        }
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

fn should_hoist_builtin(b: Builtin) -> bool {
    !matches!(b, Builtin::Soi | Builtin::Eoi | Builtin::Any)
}

fn builtin_matcher_expr(b: Builtin) -> String {
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

fn expr_has_insensitive_literal(expr: &Expr) -> bool {
    match expr {
        Expr::InsensitiveLiteral(_) => true,
        Expr::Sequence(items) | Expr::Choice(items) => {
            items.iter().any(expr_has_insensitive_literal)
        }
        Expr::Prefix { expr, .. } | Expr::Postfix { expr, .. } => {
            expr_has_insensitive_literal(expr)
        }
        Expr::Empty
        | Expr::Builtin(_)
        | Expr::RuleRef(_)
        | Expr::Literal(_)
        | Expr::Range { .. } => false,
    }
}

fn expr_needs_bounded_repeat(expr: &Expr) -> bool {
    match expr {
        Expr::Postfix { expr, op } => {
            let bounded = match op {
                PostfixOp::RepeatExact(_) | PostfixOp::RepeatMax(_) | PostfixOp::RepeatMinMax(_, _) => {
                    true
                }
                PostfixOp::RepeatMin(n) => *n > 0,
                PostfixOp::Optional
                | PostfixOp::Repeat
                | PostfixOp::RepeatOnce => false,
            };
            bounded || expr_needs_bounded_repeat(expr)
        }
        Expr::Sequence(items) | Expr::Choice(items) => {
            items.iter().any(expr_needs_bounded_repeat)
        }
        Expr::Prefix { expr, .. } => expr_needs_bounded_repeat(expr),
        Expr::Empty
        | Expr::Builtin(_)
        | Expr::RuleRef(_)
        | Expr::Literal(_)
        | Expr::InsensitiveLiteral(_)
        | Expr::Range { .. } => false,
    }
}

fn expr_needs_ws_repeat_helper(expr: &Expr) -> bool {
    match expr {
        Expr::Postfix { expr, op } => {
            matches!(
                op,
                PostfixOp::Repeat | PostfixOp::RepeatMin(0)
            ) || expr_needs_ws_repeat_helper(expr)
        }
        Expr::Sequence(items) | Expr::Choice(items) => {
            items.iter().any(expr_needs_ws_repeat_helper)
        }
        Expr::Prefix { expr, .. } => expr_needs_ws_repeat_helper(expr),
        Expr::Empty
        | Expr::Builtin(_)
        | Expr::RuleRef(_)
        | Expr::Literal(_)
        | Expr::InsensitiveLiteral(_)
        | Expr::Range { .. } => false,
    }
}

fn expr_needs_ws_repeat_once_helper(expr: &Expr) -> bool {
    match expr {
        Expr::Postfix { expr, op } => {
            matches!(op, PostfixOp::RepeatOnce) || expr_needs_ws_repeat_once_helper(expr)
        }
        Expr::Sequence(items) | Expr::Choice(items) => {
            items.iter().any(expr_needs_ws_repeat_once_helper)
        }
        Expr::Prefix { expr, .. } => expr_needs_ws_repeat_once_helper(expr),
        Expr::Empty
        | Expr::Builtin(_)
        | Expr::RuleRef(_)
        | Expr::Literal(_)
        | Expr::InsensitiveLiteral(_)
        | Expr::Range { .. } => false,
    }
}

pub fn bind_var_name(rule_name: &str) -> String {
    format!("{}_val", sanitize_ident(rule_name))
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

    #[test]
    fn dominant_sigil_marks_repeated_rule_refs_as_multiple() {
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
        let sigils = dominant_sigil_map(&expr);
        assert_eq!(sigils.get("item"), Some(&BindSigil::Multiple));
    }

    #[test]
    fn dominant_sigil_uses_optional_for_partial_one_of() {
        let expr = Expr::Choice(vec![
            Expr::Literal(" ".to_string()),
            Expr::RuleRef("newline".to_string()),
        ]);
        let sigils = dominant_sigil_map(&expr);
        assert_eq!(sigils.get("newline"), Some(&BindSigil::Optional));
    }

    #[test]
    fn extract_rule_source_comments_collects_multiline_rules() {
        let source = "expr = {\n    a ~ b\n}\n";
        let comments = extract_rule_source_comments(source);
        assert!(comments["expr"].contains("// expr = {"));
        assert!(comments["expr"].contains("//     a ~ b"));
        assert!(comments["expr"].contains("// }"));
    }

    #[test]
    fn substitute_bind_placeholders_replaces_nested_parens() {
        let source = "(bind!(item.clone(), *item_val), repeat_ws((',', bind!(item.clone(), *item_val)), ws.clone()))";
        let (substituted, originals) = substitute_bind_placeholders(source);
        assert_eq!(originals.len(), 2);
        assert!(substituted.contains("__pest_fmt_bind_0__"));
        assert!(substituted.contains("__pest_fmt_bind_1__"));
        assert!(!substituted.contains("bind!("));
        assert_eq!(restore_bind_placeholders(&substituted, &originals), source);
    }

    #[test]
    fn format_expr_str_pretty_prints_bind_expressions() {
        let source = "(start_of_input(), ws.clone(), bind!(item.clone(), *item_val), ws.clone(), repeat_ws((',', ws.clone(), bind!(item.clone(), *item_val)), ws.clone()), ws.clone(), end_of_input())";
        let out = format_expr_str(source, 8).unwrap();
        assert!(out.lines().count() > 1, "expected multiline output, got:\n{out}");
        assert!(out.contains("bind!(item.clone(), *item_val)"));
    }
}
