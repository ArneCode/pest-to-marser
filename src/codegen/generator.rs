use std::collections::{HashMap, HashSet};

use crate::ast::{Modifier, PostfixOp, PrefixOp};
use crate::error::ConvertError;
use crate::expr::{Builtin, Expr, MatchingContext, SymKey};
use crate::normalize::{RuleDef, RuleTable};
use crate::output::{
    BindSigil, FieldKey, FieldKind, RuleOutputSpec, analyze_rule_output, build_field_init,
    emit_parsed_enum, field_bind_var, field_sigil_map, tagged_bind_var_name, variant_name,
};
use crate::scc::{
    Scc, condensation_topo, is_cyclic, partition_scc_for_recursion, recursive_arity,
};
use crate::specialize::{
    SpecializationGraph, callee_context, collect_rule_deps,
};
use crate::trivia::compute_matcher_only_rules;

use super::comments::extract_rule_source_comments;
use super::expr_analysis::{
    builtin_matcher_expr, collect_builtins, expr_has_insensitive_literal,
    expr_needs_bounded_repeat, expr_needs_ws_repeat_helper, expr_needs_ws_repeat_once_helper,
    should_hoist_builtin,
};
use super::format::{
    BodyLayout, format_construction_for_capture, format_expr_str, indent_lines,
    peel_single_postfix, render_nested_one_of, render_nested_tuple,
};
use super::import_needs::{ImportNeeds, compute_import_needs, push_braced_use_list};
use super::naming::{
    bind_var_name, binding_name_for_graph, contexts_by_rule, sanitize_ident,
};
use super::CodegenOptions;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodegenMode {
    Matcher,
}

fn sorted_scc_members(members: &[SymKey]) -> Vec<SymKey> {
    let mut sorted = members.to_vec();
    sorted.sort_by(|left, right| {
        left.rule
            .cmp(&right.rule)
            .then_with(|| format!("{:?}", left.context).cmp(&format!("{:?}", right.context)))
    });
    sorted
}

pub(crate) struct Generator<'a> {
    table: &'a RuleTable,
    graph: &'a SpecializationGraph,
    sccs: &'a [Scc],
    options: &'a CodegenOptions,
    rule_comments: HashMap<String, String>,
    sym_names: HashMap<SymKey, String>,
    cyclic_syms: HashSet<SymKey>,
    referenced_builtins: HashSet<Builtin>,
    emitted: HashSet<SymKey>,
    needs_ws_repeat_helper: bool,
    needs_ws_repeat_once_helper: bool,
    needs_ci_ch_helper: bool,
    import_needs: ImportNeeds,
    recursive_comment_emitted: bool,
    matcher_only: HashSet<String>,
}

impl<'a> Generator<'a> {
    pub(crate) fn new(
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
        for scc in sccs {
            if is_cyclic(scc) {
                for member in &scc.members {
                    cyclic_syms.insert(member.clone());
                }
            }
        }

        let mut referenced_builtins = HashSet::new();
        for sym in &graph.nodes {
            if let Some(rule) = graph.rule_map.get(&sym.rule) {
                collect_builtins(&rule.expr, &mut referenced_builtins);
            }
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

        let matcher_only = compute_matcher_only_rules(table, graph);

        Self {
            table,
            graph,
            sccs,
            options,
            rule_comments,
            sym_names,
            cyclic_syms,
            referenced_builtins,
            emitted: HashSet::new(),
            needs_ws_repeat_helper,
            needs_ws_repeat_once_helper,
            needs_ci_ch_helper,
            import_needs,
            recursive_comment_emitted: false,
            matcher_only,
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
        let mut parser = vec!["Parser".to_string()];
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

    pub(crate) fn emit(&mut self) -> Result<String, ConvertError> {
        let mut out = String::new();
        self.emit_imports(&mut out);
        if self.needs_ws_repeat_helper {
            if self.options.emit_comments {
                out.push_str(
                    "// Inserts whitespace between repetitions, but not before the first item.\n\
                     // This keeps `X*` equivalent to the source grammar while avoiding duplicated\n\
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
                    "// `X+` requires a first item, then whitespace only between later repetitions.\n\
                     // This helper preserves that shape without duplicating the generated matcher\n\
                     // body for `X`.\n",
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
                    "// Case-insensitive string literals match ASCII letters without regard to case.\n",
                );
            }
            out.push_str(
                "fn ci_ch<'src, MRes>(c: char) -> impl Matcher<'src, &'src str, MRes> + Clone {\n\
                 \x20   one_of((c.to_ascii_lowercase(), c.to_ascii_uppercase()))\n\
                 }\n\n",
            );
        }
        if self.options.emit_comments {
            out.push_str(
                "// Typed parse tree returned by `grammar()`. Each grammar rule becomes a variant;\n\
                 // labeled bindings become struct fields, and leaf rules store their matched slice\n\
                 // as `value`.\n",
            );
        }
        out.push_str(&emit_parsed_enum(&self.table.rules, &self.matcher_only));
        if self.options.emit_comments {
            let fn_name = sanitize_ident(&self.options.function_name);
            out.push_str(&format!(
                "// Returns a complete parser for this grammar.\n\
                 // Usage: {fn_name}().parse_str(src)  →  Ok((Parsed, errors))\n",
            ));
        }
        out.push_str(&format!(
            "pub fn {}<'src>() -> impl Parser<'src, &'src str, Output = Parsed<'src>> + Clone {{\n",
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
                for member in sorted_scc_members(&scc.members) {
                    self.emit_acyclic_sym(&mut out, &member)?;
                }
            }
        }

        if !ws_emitted
            && self
                .graph
                .nodes
                .iter()
                .any(|sym| sym.context == MatchingContext::NormalWs)
        {
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

        if self.matcher_only.contains(&sym.rule) {
            self.emit_matcher_sym(out, sym)?;
        } else {
            self.emit_acyclic_sym(out, sym)?;
        }
        visiting.remove(sym);
        Ok(())
    }

    fn emit_matcher_sym(&mut self, out: &mut String, sym: &SymKey) -> Result<(), ConvertError> {
        if self.emitted.contains(sym) || self.cyclic_syms.contains(sym) {
            return Ok(());
        }
        if !self.sym_names.contains_key(sym) {
            let contexts_by_rule = contexts_by_rule(&self.graph.nodes);
            self.sym_names
                .insert(sym.clone(), binding_name_for_graph(sym, &contexts_by_rule));
        }
        let name = self.sym_names[sym].clone();
        self.emit_rule_comment(out, &sym.rule, "    ");
        let body = self.gen_matcher_body(sym, None, 4, BodyLayout::AssignmentContinuation)?;
        let body = match body.split_once('\n') {
            Some((first, rest)) if !rest.is_empty() => format!("{}\n{rest}", first.trim_start()),
            _ => body.trim_start().to_string(),
        };
        out.push_str(&format!("    let {name} = {body};\n\n"));
        self.emitted.insert(sym.clone());
        Ok(())
    }

    fn ws_unit_ref(&self, rule_name: &str) -> Result<String, ConvertError> {
        let sym = SymKey {
            rule: rule_name.to_string(),
            context: MatchingContext::AtomicNoWs,
        };
        let name = self.sym_names[&sym].clone();
        if self.matcher_only.contains(rule_name) {
            Ok(format!("{name}.clone()"))
        } else {
            Ok(format!("{name}.clone().ignore_result()"))
        }
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
                let ws_ref = self.ws_unit_ref("WHITESPACE")?;
                let comment_ref = self.ws_unit_ref("COMMENT")?;
                format!("one_of(({ws_ref}, {comment_ref}))")
            }
            (true, false) => self.ws_unit_ref("WHITESPACE")?,
            (false, true) => self.ws_unit_ref("COMMENT")?,
            (false, false) => unreachable!(),
        };
        let formatted = format_expr_str(&inner, 8)?;
        if self.options.emit_comments {
            out.push_str(
                "    // Pest injects WHITESPACE (and COMMENT) between every `~` in non-atomic rules.\n\
                 \x20   // ws.clone() appears between sequence elements throughout this file for that reason.\n",
            );
        }
        out.push_str(&format!("    let ws = many(\n{formatted}\n    );\n\n"));
        Ok(())
    }

    fn emit_acyclic_sym(&mut self, out: &mut String, sym: &SymKey) -> Result<(), ConvertError> {
        if self.emitted.contains(sym) || self.cyclic_syms.contains(sym) {
            return Ok(());
        }
        if self.matcher_only.contains(&sym.rule) {
            return self.emit_matcher_sym(out, sym);
        }
        if !self.sym_names.contains_key(sym) {
            let contexts_by_rule = contexts_by_rule(&self.graph.nodes);
            self.sym_names
                .insert(sym.clone(), binding_name_for_graph(sym, &contexts_by_rule));
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

    fn rule_map_refs(&self) -> HashMap<String, &RuleDef> {
        self.graph
            .rule_map
            .iter()
            .map(|(name, rule)| (name.clone(), rule))
            .collect()
    }

    fn build_variant_construction(rule_name: &str, spec: &RuleOutputSpec, expr: &Expr) -> String {
        let variant = variant_name(rule_name);
        if spec.is_leaf {
            return format!("Parsed::{variant} {{ value }}");
        }
        let fields = spec
            .fields
            .iter()
            .map(|field| {
                let bind_var = field_bind_var(field, expr);
                format!("{}: {}", field.name, build_field_init(field, &bind_var))
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("Parsed::{variant} {{ {fields} }}")
    }

    fn gen_body(
        &self,
        sym: &SymKey,
        recursive_members: Option<&[SymKey]>,
        base_column: usize,
        layout: BodyLayout,
    ) -> Result<String, ConvertError> {
        let rule = &self.graph.rule_map[&sym.rule];
        let rule_map = self.rule_map_refs();
        let spec = analyze_rule_output(&rule.expr, &rule_map);
        let sigil_map = field_sigil_map(&spec);
        let inner = self.gen_expr(
            &rule.expr,
            sym.context,
            recursive_members,
            CodegenMode::Matcher,
            &spec,
            &sigil_map,
            false,
            false,
        );
        let inner_column = match layout {
            BodyLayout::AssignmentContinuation => base_column + 4,
            BodyLayout::Block => 4,
        };
        let close_indent = match layout {
            BodyLayout::AssignmentContinuation => " ".repeat(base_column),
            BodyLayout::Block => String::new(),
        };
        let formatted_grammar = if spec.is_leaf {
            let bind_slice_expr = format!("bind_slice!({inner}, value as &'src str)");
            format_expr_str(&bind_slice_expr, inner_column)?
        } else {
            format_expr_str(&inner, inner_column)?
        };
        let construction = Self::build_variant_construction(&sym.rule, &spec, &rule.expr);
        let formatted_construction = format_construction_for_capture(&construction, inner_column)?;
        let capture =
            format!("capture!(\n{formatted_grammar} => {formatted_construction}\n{close_indent})");
        Ok(match layout {
            BodyLayout::AssignmentContinuation => capture,
            BodyLayout::Block => indent_lines(&capture, base_column),
        })
    }

    fn gen_matcher_body(
        &self,
        sym: &SymKey,
        recursive_members: Option<&[SymKey]>,
        base_column: usize,
        layout: BodyLayout,
    ) -> Result<String, ConvertError> {
        let rule = &self.graph.rule_map[&sym.rule];
        let rule_map = self.rule_map_refs();
        let spec = analyze_rule_output(&rule.expr, &rule_map);
        let empty_spec = RuleOutputSpec {
            fields: Vec::new(),
            is_leaf: spec.is_leaf,
        };
        let empty_sigil = HashMap::new();
        let inner = self.gen_expr(
            &rule.expr,
            sym.context,
            recursive_members,
            CodegenMode::Matcher,
            &empty_spec,
            &empty_sigil,
            false,
            true,
        );
        let inner_column = match layout {
            BodyLayout::AssignmentContinuation => base_column + 4,
            BodyLayout::Block => 4,
        };
        let formatted = format_expr_str(&inner, inner_column)?;
        Ok(match layout {
            BodyLayout::AssignmentContinuation => formatted,
            BodyLayout::Block => indent_lines(&formatted, base_column),
        })
    }

    fn trace_bind_slice(&self, reference: &str, sigil_prefix: &str, bind_name: &str) -> String {
        format!("bind_slice!({reference}, {sigil_prefix}{bind_name} as &'src str)")
    }

    fn gen_expr(
        &self,
        expr: &Expr,
        ctx: MatchingContext,
        recursive_members: Option<&[SymKey]>,
        mode: CodegenMode,
        spec: &RuleOutputSpec,
        sigil_map: &HashMap<FieldKey, BindSigil>,
        in_lookahead: bool,
        suppress_bind: bool,
    ) -> String {
        match (expr, mode) {
            (Expr::Empty, _) => "()".to_string(),
            (Expr::Literal(s), _) => self.gen_string_matcher(s),
            (Expr::InsensitiveLiteral(s), _) => self.gen_insensitive_matcher(s),
            (Expr::Builtin(b), _) => self.gen_builtin_matcher(*b),
            (Expr::Range { start, end }, _) => format!("{start:?}..={end:?}"),
            (Expr::RuleRef(name), mode) => self.gen_rule_ref(
                name,
                ctx,
                recursive_members,
                mode,
                sigil_map,
                in_lookahead,
                suppress_bind,
            ),
            (Expr::Tagged { tag, expr }, _) => {
                if in_lookahead {
                    return self.gen_expr(
                        expr,
                        ctx,
                        recursive_members,
                        mode,
                        spec,
                        sigil_map,
                        true,
                        suppress_bind,
                    );
                }
                let key = FieldKey::Tag(tag.clone());
                let sigil = sigil_map.get(&key).copied().unwrap_or(BindSigil::Plain);
                let field_kind = spec
                    .fields
                    .iter()
                    .find(|field| field.key == key)
                    .map(|field| field.kind)
                    .unwrap_or(FieldKind::Slice);
                let (core_expr, postfix) = peel_single_postfix(expr);
                let bind_name = tagged_bind_var_name(tag, core_expr);
                match field_kind {
                    FieldKind::ParsedChild => {
                        let rule_name = crate::output::unwrap_to_rule_ref(core_expr)
                            .expect("tagged parsed-child field must refer to a rule");
                        let rule = &self.graph.rule_map[rule_name];
                        let sym = SymKey {
                            rule: rule_name.to_string(),
                            context: callee_context(ctx, rule.modifier.as_ref()),
                        };
                        let reference = self.sym_ref(&sym, recursive_members);
                        let bind = self.trace_bind(&reference, sigil.prefix(), &bind_name, rule);
                        match postfix {
                            None => bind,
                            Some(op) => self.gen_matcher_postfix(&bind, op, ctx),
                        }
                    }
                    FieldKind::Slice => {
                        let inner_matcher = self.gen_expr(
                            core_expr,
                            ctx,
                            recursive_members,
                            mode,
                            spec,
                            sigil_map,
                            false,
                            true,
                        );
                        let bind =
                            self.trace_bind_slice(&inner_matcher, sigil.prefix(), &bind_name);
                        match postfix {
                            None => bind,
                            Some(op) => self.gen_matcher_postfix(&bind, op, ctx),
                        }
                    }
                }
            }
            (Expr::Sequence(items), mode) => self.gen_sequence(
                items,
                ctx,
                recursive_members,
                mode,
                spec,
                sigil_map,
                in_lookahead,
                suppress_bind,
            ),
            (Expr::Choice(items), mode) => {
                let parts: Vec<_> = items
                    .iter()
                    .map(|item| {
                        self.gen_expr(
                            item,
                            ctx,
                            recursive_members,
                            mode,
                            spec,
                            sigil_map,
                            in_lookahead,
                            suppress_bind,
                        )
                    })
                    .collect();
                render_nested_one_of(&parts)
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
                    spec,
                    sigil_map,
                    in_lookahead || in_la,
                    suppress_bind,
                );
                match op {
                    PrefixOp::PositivePredicate => format!("positive_lookahead({inner})"),
                    PrefixOp::NegativePredicate => format!("negative_lookahead({inner})"),
                }
            }
            (Expr::Postfix { expr, op }, _) => self.gen_postfix(
                expr,
                op,
                ctx,
                recursive_members,
                spec,
                sigil_map,
                in_lookahead,
                suppress_bind,
            ),
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
        spec: &RuleOutputSpec,
        sigil_map: &HashMap<FieldKey, BindSigil>,
        in_lookahead: bool,
        suppress_bind: bool,
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
                spec,
                sigil_map,
                in_lookahead,
                suppress_bind,
            ));
        }
        if mode == CodegenMode::Matcher {
            render_nested_tuple(&parts)
        } else {
            unreachable!("sequences are emitted as matcher tuples")
        }
    }

    fn gen_matcher_postfix(&self, inner: &str, op: &PostfixOp, ctx: MatchingContext) -> String {
        match op {
            PostfixOp::Optional => format!("optional({inner})"),
            PostfixOp::Repeat => self.gen_unbounded_repeat(inner, ctx, false),
            PostfixOp::RepeatOnce => self.gen_unbounded_repeat(inner, ctx, true),
            PostfixOp::RepeatExact(n) => self.gen_bounded_repeat(inner, ctx, *n, Some(*n)),
            PostfixOp::RepeatMin(n) if *n == 0 => self.gen_unbounded_repeat(inner, ctx, false),
            PostfixOp::RepeatMin(n) => self.gen_bounded_repeat(inner, ctx, *n, None),
            PostfixOp::RepeatMax(n) => self.gen_bounded_repeat(inner, ctx, 0, Some(*n)),
            PostfixOp::RepeatMinMax(min, max) if *min == 0 => {
                self.gen_bounded_repeat(inner, ctx, 0, Some(*max))
            }
            PostfixOp::RepeatMinMax(min, max) => {
                self.gen_bounded_repeat(inner, ctx, *min, Some(*max))
            }
        }
    }

    fn gen_postfix(
        &self,
        expr: &Expr,
        op: &PostfixOp,
        ctx: MatchingContext,
        recursive_members: Option<&[SymKey]>,
        spec: &RuleOutputSpec,
        sigil_map: &HashMap<FieldKey, BindSigil>,
        in_lookahead: bool,
        suppress_bind: bool,
    ) -> String {
        let inner = self.gen_expr(
            expr,
            ctx,
            recursive_members,
            CodegenMode::Matcher,
            spec,
            sigil_map,
            in_lookahead,
            suppress_bind,
        );
        self.gen_matcher_postfix(&inner, op, ctx)
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
                return format!("repeat_ws({inner}, {ws})");
            }
            return format!("repeat_one_or_more_ws({inner}, {ws})");
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
        sigil_map: &HashMap<FieldKey, BindSigil>,
        in_lookahead: bool,
        suppress_bind: bool,
    ) -> String {
        if let Some(rule) = self.graph.rule_map.get(name) {
            let callee_ctx = callee_context(ctx, rule.modifier.as_ref());
            let sym = SymKey {
                rule: name.to_string(),
                context: callee_ctx,
            };
            let reference = self.sym_ref(&sym, recursive_members);
            if rule.modifier == Some(Modifier::Silent) {
                return reference;
            }
            if in_lookahead || suppress_bind {
                if self.matcher_only.contains(name) {
                    return reference;
                }
                return format!("{reference}.ignore_result()");
            }
            let key = FieldKey::Rule(name.to_string());
            let sigil = sigil_map.get(&key).copied().unwrap_or(BindSigil::Plain);
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
            render_nested_tuple(&parts)
        }
    }

    fn gen_builtin_matcher(&self, b: Builtin) -> String {
        if should_hoist_builtin(b) && self.referenced_builtins.contains(&b) {
            return format!("{}.clone()", sanitize_ident(b.name()));
        }
        builtin_matcher_expr(b)
    }
}
