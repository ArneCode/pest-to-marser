use marser::parser::Parser;
use pest_to_marser::{ConvertError, ConvertOptions, convert_pest_source, get_pest_grammar};

#[test]
fn meta_grammar_parses_fully() {
    let src = include_str!("fixtures/grammar.pest");
    get_pest_grammar()
        .parse_str(src)
        .expect("meta grammar should parse");
}

#[test]
fn rejects_duplicate_rules() {
    let src = r#"a = { "x" } a = { "y" }"#;
    let err = convert_pest_source(
        src,
        &ConvertOptions {
            entry_rule: "a".to_string(),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(matches!(err[0], ConvertError::DuplicateRule { .. }));
}

#[test]
fn rejects_left_recursion() {
    let src = include_str!("fixtures/left_rec.pest");
    let err = convert_pest_source(
        src,
        &ConvertOptions {
            entry_rule: "a".to_string(),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        err.iter()
            .any(|e| matches!(e, ConvertError::LeftRecursion { .. }))
    );
}

#[test]
fn rejects_unsafe_repeat() {
    let src = r#"main = { ("")* }"#;
    let err = convert_pest_source(
        src,
        &ConvertOptions {
            entry_rule: "main".to_string(),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(err.iter().any(|e| matches!(
        e,
        ConvertError::NonFailingRepetition { .. } | ConvertError::NonProgressingRepetition { .. }
    )));
}

#[test]
fn simple_grammar_generates_rust() {
    let src = include_str!("fixtures/simple.pest");
    let code = convert_pest_source(
        src,
        &ConvertOptions {
            entry_rule: "main".to_string(),
            ..Default::default()
        },
    )
    .expect("simple grammar should convert");
    assert!(code.contains("pub fn grammar"));
    assert!(code.contains("start_of_input()"));
    assert!(code.contains("end_of_input()"));
}

#[test]
fn calc_grammar_generates_recursive_block() {
    let src = include_str!("fixtures/calc.pest");
    let code = convert_pest_source(
        src,
        &ConvertOptions {
            entry_rule: "expr".to_string(),
            ..Default::default()
        },
    )
    .expect("calc grammar should convert");
    assert!(code.contains("recursive("));
    assert!(!code.contains("recursive2"));
    assert!(!code.contains("recursive3"));
    assert!(code.contains("let factor ="));
    assert!(code.contains("let term ="));
}

#[test]
fn repeat_once_only_emits_repeat_once_helper() {
    let src = r#"
WHITESPACE = _{ " " }
main = { SOI ~ "a"+ ~ EOI }
"#;
    let code = convert_pest_source(
        src,
        &ConvertOptions {
            entry_rule: "main".to_string(),
            ..Default::default()
        },
    )
    .expect("repeat-once grammar should convert");

    assert!(code.contains("fn repeat_one_or_more_ws"));
    assert!(!code.contains("fn repeat_ws"));
}

mod generated_calc {
    include!("generated/calc.rs");
}

#[test]
fn generated_calc_compiles_and_parses() {
    use generated_calc::grammar;

    assert!(grammar().parse_str("1").is_ok());
    assert!(grammar().parse_str("1+2").is_ok());
    assert!(grammar().parse_str("1+2*3").is_ok());
    assert!(grammar().parse_str("(1+2)*3").is_ok());
    assert!(grammar().parse_str("1 + 2 * 3").is_ok());
    assert!(grammar().parse_str("").is_err());
    assert!(grammar().parse_str("1+").is_err());
}

mod e2e_calc {
    use marser::parser::Parser as MarserParser;
    use pest::Parser;
    use pest_derive::Parser as PestDerive;

    #[derive(PestDerive)]
    #[grammar = "tests/fixtures/calc.pest"]
    struct CalcPest;

    mod generated {
        include!("generated/calc.rs");
    }

    fn pest_accepts(input: &str) -> bool {
        CalcPest::parse(Rule::expr, input).is_ok()
    }

    fn marser_accepts(input: &str) -> bool {
        generated::grammar().parse_str(input).is_ok()
    }

    #[test]
    fn accept_reject_corpora_match() {
        let inputs = [
            "1",
            "1+2",
            "1+2*3",
            "(1+2)*3",
            "1 + 2 * 3",
            "10/2-3",
            "",
            "((1)",
        ];
        for input in inputs {
            assert_eq!(
                pest_accepts(input),
                marser_accepts(input),
                "mismatch on input: {input:?}"
            );
        }
    }
}

mod generated_simple {
    include!("generated/simple.rs");
}

#[test]
fn generated_simple_compiles_and_parses() {
    use generated_simple::grammar;

    assert!(grammar().parse_str("a=1").is_ok());
    assert!(grammar().parse_str("a=1,b=2").is_ok());
    assert!(grammar().parse_str("a=1, b=2").is_ok());
    assert!(grammar().parse_str("bad").is_err());
}

mod e2e_simple {
    use marser::parser::Parser as MarserParser;
    use pest::Parser;
    use pest_derive::Parser as PestDerive;

    #[derive(PestDerive)]
    #[grammar = "tests/fixtures/simple.pest"]
    struct SimplePest;

    mod generated {
        include!("generated/simple.rs");
    }

    fn pest_accepts(input: &str) -> bool {
        SimplePest::parse(Rule::main, input).is_ok()
    }

    fn marser_accepts(input: &str) -> bool {
        generated::grammar().parse_str(input).is_ok()
    }

    #[test]
    fn accept_reject_corpora_match() {
        let inputs = [
            "a=1",
            "a=1,b=2",
            "a=1, b=2",
            "x=9,y=8,z=7",
            "",
            "a=",
            "1=1",
            "a=1,",
            "a=1,,b=2",
            "a=1 b=2",
        ];
        for input in inputs {
            assert_eq!(
                pest_accepts(input),
                marser_accepts(input),
                "mismatch on input: {input:?}"
            );
        }
    }
}
