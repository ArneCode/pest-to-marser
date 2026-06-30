use std::collections::{HashMap, HashSet};

use crate::ast::{Modifier, PostfixOp, PrefixOp};
use crate::expr::Expr;
use crate::normalize::RuleDef;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindSigil {
    Plain,
    Optional,
    Multiple,
}

impl BindSigil {
    pub fn prefix(self) -> &'static str {
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FieldKey {
    Tag(String),
    Rule(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    ParsedChild,
    Slice,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldSpec {
    pub key: FieldKey,
    pub name: String,
    pub kind: FieldKind,
    pub sigil: BindSigil,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleOutputSpec {
    pub fields: Vec<FieldSpec>,
    pub is_leaf: bool,
}

fn occurrence_class(wrapping: Option<WrappingPostfix>) -> OccurrenceClass {
    match wrapping {
        Some(WrappingPostfix::Optional) => OccurrenceClass::Optional,
        Some(WrappingPostfix::Repeat) => OccurrenceClass::Multiple,
        None => OccurrenceClass::Plain,
    }
}

fn tagged_inner_wrapping(expr: &Expr) -> Option<WrappingPostfix> {
    match expr {
        Expr::Postfix { op, .. } => Some(match op {
            PostfixOp::Optional => WrappingPostfix::Optional,
            PostfixOp::Repeat
            | PostfixOp::RepeatOnce
            | PostfixOp::RepeatExact(_)
            | PostfixOp::RepeatMin(_)
            | PostfixOp::RepeatMax(_)
            | PostfixOp::RepeatMinMax(_, _) => WrappingPostfix::Repeat,
        }),
        _ => None,
    }
}

fn merge_choice_occurrence_classes(per_alt: &[Vec<OccurrenceClass>]) -> OccurrenceClass {
    let sigils: Vec<BindSigil> = per_alt
        .iter()
        .map(|classes| dominant_sigil_from_occurrences(classes))
        .collect();
    if sigils
        .iter()
        .any(|sigil| matches!(sigil, BindSigil::Multiple))
    {
        OccurrenceClass::Multiple
    } else if sigils
        .iter()
        .any(|sigil| matches!(sigil, BindSigil::Optional))
    {
        OccurrenceClass::Optional
    } else {
        OccurrenceClass::Plain
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

fn field_name_for_key(key: &FieldKey) -> String {
    match key {
        FieldKey::Tag(tag) => sanitize_field_name(tag),
        FieldKey::Rule(rule) => format!("{}_val", sanitize_field_name(rule)),
    }
}

pub fn sanitize_field_name(name: &str) -> String {
    if RUST_KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
    "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct",
    "super", "trait", "true", "type", "unsafe", "use", "where", "while", "yield",
];

fn is_defined_rule(name: &str, rules: &HashMap<String, &RuleDef>) -> bool {
    rules.contains_key(name)
}

fn tagged_field_kind(expr: &Expr, rules: &HashMap<String, &RuleDef>) -> FieldKind {
    match unwrap_to_rule_ref(expr) {
        Some(name) if is_defined_rule(name, rules) => {
            if rules
                .get(name)
                .is_some_and(|rule| rule.modifier == Some(Modifier::Silent))
            {
                FieldKind::Slice
            } else {
                FieldKind::ParsedChild
            }
        }
        _ => FieldKind::Slice,
    }
}

pub fn unwrap_to_rule_ref(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::RuleRef(name) => Some(name),
        Expr::Postfix { expr, .. } | Expr::Prefix { expr, .. } | Expr::Tagged { expr, .. } => {
            unwrap_to_rule_ref(expr)
        }
        _ => None,
    }
}

pub fn tagged_rule_ref_for_tag(expr: &Expr, tag: &str) -> Option<String> {
    match expr {
        Expr::Tagged { tag: t, expr } if t == tag => {
            unwrap_to_rule_ref(expr).map(|name| name.to_string())
        }
        Expr::Sequence(items) | Expr::Choice(items) => items
            .iter()
            .find_map(|item| tagged_rule_ref_for_tag(item, tag)),
        Expr::Prefix { expr, .. } | Expr::Postfix { expr, .. } => {
            tagged_rule_ref_for_tag(expr, tag)
        }
        _ => None,
    }
}

pub fn tagged_bind_var_name(tag: &str, inner_expr: &Expr) -> String {
    let tag_ident = sanitize_field_name(tag);
    match unwrap_to_rule_ref(inner_expr) {
        Some(rule) if rule == tag => format!("{tag_ident}_val"),
        _ => tag_ident,
    }
}

pub fn field_bind_var(field: &FieldSpec, expr: &Expr) -> String {
    match &field.key {
        FieldKey::Rule(rule) => format!("{}_val", sanitize_field_name(rule)),
        FieldKey::Tag(tag) => tagged_rule_ref_for_tag(expr, tag).map_or_else(
            || sanitize_field_name(tag),
            |rule| tagged_bind_var_name(tag, &Expr::RuleRef(rule)),
        ),
    }
}

fn collect_field_occurrences(
    expr: &Expr,
    rules: &HashMap<String, &RuleDef>,
    in_lookahead: bool,
    wrapping: Option<WrappingPostfix>,
    order: &mut Vec<FieldKey>,
    out: &mut HashMap<FieldKey, (FieldKind, Vec<OccurrenceClass>)>,
) {
    match expr {
        Expr::Tagged { tag, expr } => {
            if in_lookahead {
                collect_field_occurrences(expr, rules, true, wrapping, order, out);
                return;
            }
            let key = FieldKey::Tag(tag.clone());
            let kind = tagged_field_kind(expr, rules);
            if !out.contains_key(&key) {
                order.push(key.clone());
            }
            let field_wrapping = tagged_inner_wrapping(expr).or(wrapping);
            out.entry(key)
                .or_insert((kind, Vec::new()))
                .1
                .push(occurrence_class(field_wrapping));
        }
        Expr::RuleRef(name) => {
            if in_lookahead || !is_defined_rule(name, rules) {
                return;
            }
            if rules
                .get(name)
                .is_some_and(|rule| rule.modifier == Some(Modifier::Silent))
            {
                return;
            }
            let key = FieldKey::Rule(name.clone());
            if !out.contains_key(&key) {
                order.push(key.clone());
            }
            out.entry(key)
                .or_insert((FieldKind::ParsedChild, Vec::new()))
                .1
                .push(occurrence_class(wrapping));
        }
        Expr::Prefix { op, expr } => {
            let in_la = matches!(
                op,
                PrefixOp::PositivePredicate | PrefixOp::NegativePredicate
            );
            collect_field_occurrences(expr, rules, in_lookahead || in_la, wrapping, order, out);
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
            collect_field_occurrences(expr, rules, in_lookahead, inner_wrapping, order, out);
        }
        Expr::Sequence(items) => {
            for item in items {
                collect_field_occurrences(item, rules, in_lookahead, wrapping, order, out);
            }
        }
        Expr::Choice(items) => {
            let per_alt: Vec<(
                Vec<FieldKey>,
                HashMap<FieldKey, (FieldKind, Vec<OccurrenceClass>)>,
            )> = items
                .iter()
                .map(|item| {
                    let mut alt_order = Vec::new();
                    let mut alt_map = HashMap::new();
                    collect_field_occurrences(
                        item,
                        rules,
                        in_lookahead,
                        wrapping,
                        &mut alt_order,
                        &mut alt_map,
                    );
                    (alt_order, alt_map)
                })
                .collect();
            let num_alts = per_alt.len();
            let mut ordered_keys = Vec::new();
            let mut seen_keys = HashSet::new();
            for (alt_order, _) in &per_alt {
                for key in alt_order {
                    if seen_keys.insert(key.clone()) {
                        ordered_keys.push(key.clone());
                    }
                }
            }
            for key in ordered_keys {
                let alts_containing = per_alt.iter().filter(|(_, m)| m.contains_key(&key)).count();
                let kind = per_alt
                    .iter()
                    .find_map(|(_, m)| m.get(&key).map(|(kind, _)| *kind))
                    .unwrap_or(FieldKind::Slice);
                if alts_containing < num_alts {
                    if !out.contains_key(&key) {
                        order.push(key.clone());
                    }
                    out.entry(key)
                        .or_insert((kind, Vec::new()))
                        .1
                        .push(OccurrenceClass::Optional);
                } else {
                    if !out.contains_key(&key) {
                        order.push(key.clone());
                    }
                    let alt_class_lists: Vec<Vec<OccurrenceClass>> = per_alt
                        .iter()
                        .filter_map(|(_, alt_map)| {
                            alt_map.get(&key).map(|(_, classes)| classes.clone())
                        })
                        .collect();
                    out.entry(key.clone())
                        .or_insert((kind, Vec::new()))
                        .1
                        .push(merge_choice_occurrence_classes(&alt_class_lists));
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

pub fn analyze_rule_output(expr: &Expr, rules: &HashMap<String, &RuleDef>) -> RuleOutputSpec {
    let mut order = Vec::new();
    let mut occurrences: HashMap<FieldKey, (FieldKind, Vec<OccurrenceClass>)> = HashMap::new();
    collect_field_occurrences(expr, rules, false, None, &mut order, &mut occurrences);

    let fields = order
        .into_iter()
        .filter_map(|key| {
            let (kind, classes) = occurrences.get(&key)?;
            Some(FieldSpec {
                name: field_name_for_key(&key),
                sigil: dominant_sigil_from_occurrences(classes),
                kind: *kind,
                key,
            })
        })
        .collect::<Vec<_>>();

    RuleOutputSpec {
        is_leaf: fields.is_empty(),
        fields,
    }
}

pub fn field_sigil_map(spec: &RuleOutputSpec) -> HashMap<FieldKey, BindSigil> {
    spec.fields
        .iter()
        .map(|field| (field.key.clone(), field.sigil))
        .collect()
}

pub fn variant_name(rule_name: &str) -> String {
    sanitize_field_name(rule_name)
}

pub fn rust_field_type(field: &FieldSpec) -> String {
    match (field.kind, field.sigil) {
        (FieldKind::ParsedChild, BindSigil::Plain) => "Box<Parsed<'src>>".to_string(),
        (FieldKind::ParsedChild, BindSigil::Optional) => "Option<Box<Parsed<'src>>>".to_string(),
        (FieldKind::ParsedChild, BindSigil::Multiple) => "Vec<Box<Parsed<'src>>>".to_string(),
        (FieldKind::Slice, BindSigil::Plain) => "&'src str".to_string(),
        (FieldKind::Slice, BindSigil::Optional) => "Option<&'src str>".to_string(),
        (FieldKind::Slice, BindSigil::Multiple) => "Vec<&'src str>".to_string(),
    }
}

pub fn build_field_init(field: &FieldSpec, bind_name: &str) -> String {
    match (field.kind, field.sigil) {
        (FieldKind::ParsedChild, BindSigil::Plain) => format!("Box::new({bind_name})"),
        (FieldKind::ParsedChild, BindSigil::Optional) => {
            format!("{bind_name}.map(Box::new)")
        }
        (FieldKind::ParsedChild, BindSigil::Multiple) => {
            format!("{bind_name}.into_iter().map(Box::new).collect()")
        }
        (FieldKind::Slice, BindSigil::Plain) => bind_name.to_string(),
        (FieldKind::Slice, BindSigil::Optional | BindSigil::Multiple) => bind_name.to_string(),
    }
}

pub fn emit_parsed_enum(rules: &[RuleDef], exclude: &HashSet<String>) -> String {
    let rule_map: HashMap<_, _> = rules.iter().map(|r| (r.name.clone(), r)).collect();
    let mut out = String::from("#[derive(Debug, Clone, PartialEq)]\n");
    out.push_str("pub enum Parsed<'src> {\n");

    for rule in rules {
        if exclude.contains(&rule.name) {
            continue;
        }
        let spec = analyze_rule_output(&rule.expr, &rule_map);
        let variant = variant_name(&rule.name);
        if spec.is_leaf {
            out.push_str(&format!("    {variant} {{ value: &'src str }},\n"));
        } else {
            out.push_str(&format!("    {variant} {{\n"));
            for field in &spec.fields {
                out.push_str(&format!(
                    "        {}: {},\n",
                    field.name,
                    rust_field_type(field)
                ));
            }
            out.push_str("    },\n");
        }
    }

    out.push_str("}\n\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::PostfixOp;
    use crate::expr::Builtin;
    use crate::normalize::RuleDef;

    fn rules_map(rules: &[RuleDef]) -> HashMap<String, &RuleDef> {
        rules.iter().map(|r| (r.name.clone(), r)).collect()
    }

    fn mk_rule(name: &str, expr: Expr) -> RuleDef {
        RuleDef {
            name: name.to_string(),
            modifier: None,
            expr,
            docs: Vec::new(),
        }
    }

    #[test]
    fn leaf_rule_has_no_fields() {
        let ident = mk_rule(
            "ident",
            Expr::Sequence(vec![
                Expr::Builtin(Builtin::AsciiAlpha),
                Expr::Postfix {
                    expr: Box::new(Expr::Builtin(Builtin::AsciiAlphanumeric)),
                    op: PostfixOp::Repeat,
                },
            ]),
        );
        let spec = analyze_rule_output(&ident.expr, &rules_map(&[ident.clone()]));
        assert!(spec.is_leaf);
        assert!(spec.fields.is_empty());
    }

    #[test]
    fn untagged_duplicate_rule_refs_merge() {
        let main = mk_rule(
            "main",
            Expr::Sequence(vec![
                Expr::RuleRef("item".to_string()),
                Expr::Postfix {
                    expr: Box::new(Expr::Sequence(vec![
                        Expr::Literal(",".to_string()),
                        Expr::RuleRef("item".to_string()),
                    ])),
                    op: PostfixOp::Repeat,
                },
            ]),
        );
        let item = mk_rule("item", Expr::Literal("x".to_string()));
        let rules = vec![main, item];
        let spec = analyze_rule_output(&rules[0].expr, &rules_map(&rules));
        assert_eq!(spec.fields.len(), 1);
        assert_eq!(spec.fields[0].name, "item_val");
        assert_eq!(spec.fields[0].sigil, BindSigil::Multiple);
    }

    #[test]
    fn tagged_duplicate_rule_refs_stay_separate() {
        let main = mk_rule(
            "main",
            Expr::Sequence(vec![
                Expr::Tagged {
                    tag: "lhs".to_string(),
                    expr: Box::new(Expr::RuleRef("ident".to_string())),
                },
                Expr::Literal("=".to_string()),
                Expr::Tagged {
                    tag: "rhs".to_string(),
                    expr: Box::new(Expr::RuleRef("ident".to_string())),
                },
            ]),
        );
        let ident = mk_rule("ident", Expr::Literal("a".to_string()));
        let rules = vec![ident.clone(), main];
        let spec = analyze_rule_output(&rules[1].expr, &rules_map(&rules));
        assert_eq!(spec.fields.len(), 2);
        assert_eq!(spec.fields[0].name, "lhs");
        assert_eq!(spec.fields[1].name, "rhs");
    }

    #[test]
    fn tagged_same_tag_in_choice_alts_is_single_field() {
        let factor = mk_rule(
            "factor",
            Expr::Choice(vec![
                Expr::Tagged {
                    tag: "inner".to_string(),
                    expr: Box::new(Expr::RuleRef("number".to_string())),
                },
                Expr::Sequence(vec![
                    Expr::Literal("(".to_string()),
                    Expr::Tagged {
                        tag: "inner".to_string(),
                        expr: Box::new(Expr::RuleRef("expr".to_string())),
                    },
                    Expr::Literal(")".to_string()),
                ]),
            ]),
        );
        let number = mk_rule("number", Expr::Builtin(Builtin::AsciiDigit));
        let expr = mk_rule("expr", Expr::RuleRef("factor".to_string()));
        let rules = vec![number, expr, factor.clone()];
        let spec = analyze_rule_output(&factor.expr, &rules_map(&rules));
        assert_eq!(spec.fields.len(), 1);
        assert_eq!(spec.fields[0].name, "inner");
        assert_eq!(spec.fields[0].sigil, BindSigil::Plain);
        assert_eq!(spec.fields[0].kind, FieldKind::ParsedChild);
    }

    #[test]
    fn tagged_optional_rule_ref_becomes_optional_field() {
        let main = mk_rule(
            "main",
            Expr::Sequence(vec![
                Expr::Tagged {
                    tag: "sign".to_string(),
                    expr: Box::new(Expr::Postfix {
                        expr: Box::new(Expr::RuleRef("sign".to_string())),
                        op: PostfixOp::Optional,
                    }),
                },
                Expr::Tagged {
                    tag: "digits".to_string(),
                    expr: Box::new(Expr::RuleRef("digits".to_string())),
                },
            ]),
        );
        let sign = mk_rule("sign", Expr::Literal("+".to_string()));
        let digits = mk_rule("digits", Expr::Builtin(Builtin::AsciiDigit));
        let rules = vec![sign, digits, main];
        let spec = analyze_rule_output(&rules[2].expr, &rules_map(&rules));
        assert_eq!(spec.fields.len(), 2);
        assert_eq!(spec.fields[0].name, "sign");
        assert_eq!(spec.fields[0].sigil, BindSigil::Optional);
        assert_eq!(spec.fields[0].kind, FieldKind::ParsedChild);
        assert_eq!(spec.fields[1].name, "digits");
        assert_eq!(spec.fields[1].sigil, BindSigil::Plain);
    }

    #[test]
    fn tagged_non_rule_becomes_slice_field() {
        let main = mk_rule(
            "main",
            Expr::Sequence(vec![
                Expr::Tagged {
                    tag: "op".to_string(),
                    expr: Box::new(Expr::Choice(vec![
                        Expr::Literal("+".to_string()),
                        Expr::Literal("-".to_string()),
                    ])),
                },
                Expr::RuleRef("number".to_string()),
            ]),
        );
        let number = mk_rule("number", Expr::Builtin(Builtin::AsciiDigit));
        let rules = vec![main, number];
        let spec = analyze_rule_output(&rules[0].expr, &rules_map(&rules));
        assert_eq!(spec.fields.len(), 2);
        assert_eq!(spec.fields[0].kind, FieldKind::Slice);
        assert_eq!(spec.fields[0].name, "op");
        assert_eq!(spec.fields[1].kind, FieldKind::ParsedChild);
    }

    #[test]
    fn emit_parsed_enum_skips_excluded_rules() {
        let rules = vec![
            mk_rule("main", Expr::Literal("a".to_string())),
            mk_rule("WHITESPACE", Expr::Literal(" ".to_string())),
        ];
        let excluded = HashSet::from(["WHITESPACE".to_string()]);
        let out = emit_parsed_enum(&rules, &excluded);
        assert!(out.contains("main {"));
        assert!(!out.contains("WHITESPACE"));
    }
}
