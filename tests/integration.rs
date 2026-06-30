use marser::parser::Parser;
use grammar_to_marser::{
    convert_grammar_source, convert_source, parse_peg_grammar, parse_pest_grammar, ConvertError,
    ConvertOptions, InputSyntax,
};
use serde::Deserialize;

#[macro_use]
mod e2e;

#[derive(Deserialize)]
struct Manifest {
    fixture: Vec<FixtureEntry>,
    #[serde(default)]
    peg_fixture: Vec<PegFixtureEntry>,
}

#[derive(Deserialize)]
struct FixtureEntry {
    pest: String,
    entry: String,
    stem: String,
}

#[derive(Deserialize)]
struct PegFixtureEntry {
    peg: String,
    entry: String,
    stem: String,
}

fn fixture_manifest() -> Manifest {
    toml::from_str(include_str!("fixtures.toml")).expect("parse fixtures.toml")
}

#[test]
fn meta_grammar_parses_fully() {
    let src = include_str!("fixtures/grammar.pest");
    parse_pest_grammar()
        .parse_str(src)
        .expect("meta grammar should parse");
}

#[test]
fn peg_meta_grammar_parses_fixtures() {
    let manifest = fixture_manifest();
    for fixture in manifest.peg_fixture {
        let src = std::fs::read_to_string(format!("tests/fixtures/{}", fixture.peg))
            .unwrap_or_else(|e| panic!("read {}: {e}", fixture.peg));
        parse_peg_grammar()
            .parse_str(&src)
            .unwrap_or_else(|e| panic!("parse {}: {e:?}", fixture.peg));
    }
}

#[test]
fn rejects_duplicate_rules() {
    let src = r#"a = { "x" } a = { "y" }"#;
    let err = convert_grammar_source(
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
    let err = convert_grammar_source(
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
    let err = convert_grammar_source(
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
fn committed_generated_snapshots_match_converter() {
    for fixture in fixture_manifest().fixture {
        let pest_path = format!("tests/fixtures/{}", fixture.pest);
        let generated_path = format!("tests/generated/{}.rs", fixture.stem);
        let src =
            std::fs::read_to_string(&pest_path).unwrap_or_else(|e| panic!("read {pest_path}: {e}"));
        let expected = std::fs::read_to_string(&generated_path)
            .unwrap_or_else(|e| panic!("read {generated_path}: {e}"));
        let actual = convert_grammar_source(
            &src,
            &ConvertOptions {
                entry_rule: fixture.entry.clone(),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("convert {}: {e:?}", fixture.pest));
        assert_eq!(
            actual, expected,
            "stale {generated_path} — run: cargo run --bin update-test-fixtures"
        );
    }
}

#[test]
fn committed_peg_generated_snapshots_match_converter() {
    for fixture in fixture_manifest().peg_fixture {
        let peg_path = format!("tests/fixtures/{}", fixture.peg);
        let generated_path = format!("tests/generated/{}.rs", fixture.stem);
        let src =
            std::fs::read_to_string(&peg_path).unwrap_or_else(|e| panic!("read {peg_path}: {e}"));
        let expected = std::fs::read_to_string(&generated_path)
            .unwrap_or_else(|e| panic!("read {generated_path}: {e}"));
        let actual = convert_source(
            &src,
            InputSyntax::Peg,
            &ConvertOptions {
                entry_rule: fixture.entry.clone(),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("convert {}: {e:?}", fixture.peg));
        assert_eq!(
            actual, expected,
            "stale {generated_path} — run: cargo run --bin update-test-fixtures"
        );
    }
}

mod e2e_calc {
    pest_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/calc.pest";
            pest = CalcPest;
            generated = calc;
            entry = expr;
            corpus = [
                accept!("1"),
                accept!("1+2"),
                accept!("1+2*3"),
                accept!("(1+2)*3"),
                accept!("1 + 2 * 3"),
                accept!("10/2-3"),
                accept!("2*3+4"),
                accept!("(1)"),
                accept!("1+2+3"),
                accept!("1/0"),
                reject!(""),
                reject!("1+"),
                reject!("1++2"),
                reject!("1 2"),
                reject!("((1)"),
            ];
            fuzz;
        }
    }
}

mod e2e_calc_number {
    pest_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/calc.pest";
            pest = CalcNumberPest;
            generated = calc_number;
            entry = number;
            corpus = [
                accept!("0"),
                accept!("42"),
                accept!("007"),
                reject!(""),
                reject!("1+2"),
                reject!("12a"),
            ];
        }
    }
}

mod e2e_simple {
    pest_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/simple.pest";
            pest = SimplePest;
            generated = simple;
            entry = main;
            corpus = [
                accept!("a=1"),
                accept!("a=1,b=2"),
                accept!("a=1, b=2"),
                accept!("x=9,y=8,z=7"),
                accept!("_a=0"),
                accept!("a=1 // comment\n,b=2"),
                reject!(""),
                reject!("a="),
                reject!("1=1"),
                reject!("a=1,"),
                reject!("a=1,,b=2"),
                reject!("a=1 b=2"),
                reject!("bad"),
            ];
            fuzz;
        }
    }
}

mod e2e_dual_trivia {
    pest_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/dual_trivia.pest";
            pest = DualTriviaPest;
            generated = dual_trivia;
            entry = main;
            corpus = [
                accept!("a\tb"),
                accept!("hello\tworld"),
                reject!("ab"),
                reject!(""),
                reject!("a\t"),
            ];
        }
    }
}

mod e2e_case_insensitive {
    pest_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/case_insensitive.pest";
            pest = CaseInsensitivePest;
            generated = case_insensitive;
            entry = main;
            corpus = [
                accept!("select from foo"),
                accept!("SELECT FROM foo"),
                accept!("SeLeCt FrOm foo"),
                accept!("select  from  bar"),
                accept!("selectfrom foo"),
                reject!("select frm foo"),
                reject!("select from"),
                reject!(""),
            ];
        }
    }
}

mod e2e_bounded_repeat {
    pest_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/bounded_repeat.pest";
            pest = BoundedRepeatPest;
            generated = bounded_repeat;
            entry = main;
            corpus = [
                accept!("aa"),
                accept!("aaa"),
                accept!("aaaa"),
                reject!("a"),
                reject!("aaaaa"),
                reject!(""),
                reject!("b"),
            ];
        }
    }
}

mod e2e_lookahead {
    pest_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/lookahead.pest";
            pest = LookaheadPest;
            generated = lookahead;
            entry = main;
            corpus = [
                reject!("aend"),
                accept!("hello end"),
                accept!("hello world end"),
                accept!("foo_bar end"),
                reject!("hello"),
                reject!("end"),
                reject!(""),
                accept!("helloend end"),
            ];
        }
    }
}

mod e2e_optional {
    pest_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/optional.pest";
            pest = OptionalPest;
            generated = optional;
            entry = main;
            corpus = [
                accept!("42"),
                accept!("+42"),
                accept!("-7"),
                accept!("0"),
                reject!(""),
                reject!("++1"),
                reject!("+"),
                reject!("4.2"),
            ];
        }
    }
}

mod e2e_ranges {
    pest_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/ranges.pest";
            pest = RangesPest;
            generated = ranges;
            entry = main;
            corpus = [
                accept!("#ff00aa"),
                accept!("#FF00AA"),
                accept!("#012345"),
                reject!("#gg0000"),
                reject!("#fff"),
                reject!("#1234567"),
                reject!(""),
                reject!("ff00aa"),
            ];
        }
    }
}

mod e2e_positive_lookahead {
    pest_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/positive_lookahead.pest";
            pest = PositiveLookaheadPest;
            generated = positive_lookahead;
            entry = main;
            corpus = [
                accept!("ab"),
                accept!("ab "),
                reject!("a"),
                reject!("abc"),
                reject!(""),
                reject!("xab"),
            ];
        }
    }
}

mod e2e_compound_atomic {
    pest_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/compound_atomic.pest";
            pest = CompoundAtomicPest;
            generated = compound_atomic;
            entry = main;
            corpus = [
                accept!("hello"),
                accept!("world"),
                reject!("hel lo"),
                reject!(""),
                reject!("123"),
            ];
        }
    }
}

mod e2e_non_atomic {
    pest_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/non_atomic.pest";
            pest = NonAtomicPest;
            generated = non_atomic;
            entry = main;
            corpus = [
                reject!("a b"),
                reject!("a  b c"),
                reject!("ab"),
                reject!("a"),
                reject!(""),
            ];
        }
    }
}

mod e2e_exact_repeat {
    pest_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/exact_repeat.pest";
            pest = ExactRepeatPest;
            generated = exact_repeat;
            entry = main;
            corpus = [
                accept!("aaa"),
                reject!("aa"),
                reject!("aaaa"),
                reject!(""),
                reject!("b"),
            ];
        }
    }
}

mod e2e_peg_simple {
    peg_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/peg_simple.peg";
            generated = peg_simple;
            entry = main;
            corpus = [
                accept!("a=1"),
                accept!("a=1,b=2"),
                accept!("x=9,y=8,z=7"),
                accept!("_a=0"),
                reject!(""),
                reject!("a="),
                reject!("1=1"),
                reject!("a=1,"),
                reject!("a=1,,b=2"),
                reject!("a=1 b=2"),
            ];
        }
    }
}

mod e2e_peg_calc {
    peg_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/peg_calc.peg";
            generated = peg_calc;
            entry = expr;
            corpus = [
                accept!("1"),
                accept!("1+2"),
                accept!("1+2*3"),
                accept!("(1+2)*3"),
                accept!("10/2-3"),
                accept!("2*3+4"),
                accept!("(1)"),
                accept!("1+2+3"),
                reject!(""),
                reject!("1+"),
                reject!("1++2"),
                reject!("1 2"),
                reject!("((1)"),
            ];
        }
    }
}

mod e2e_peg_optional {
    peg_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/peg_optional.peg";
            generated = peg_optional;
            entry = main;
            corpus = [
                accept!("42"),
                accept!("+42"),
                accept!("-7"),
                accept!("0"),
                reject!(""),
                reject!("++1"),
                reject!("+"),
                reject!("4.2"),
            ];
        }
    }
}

mod e2e_peg_lookahead {
    peg_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/peg_lookahead.peg";
            generated = peg_lookahead;
            entry = main;
            corpus = [
                reject!("aend"),
                accept!("hello end"),
                accept!("hello world end"),
                accept!("foo_bar end"),
                reject!("hello"),
                reject!("end"),
                reject!(""),
                accept!("helloend end"),
            ];
        }
    }
}

mod e2e_peg_positive_lookahead {
    peg_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/peg_positive_lookahead.peg";
            generated = peg_positive_lookahead;
            entry = main;
            corpus = [
                accept!("ab"),
                reject!("ab "),
                reject!("a"),
                reject!("abc"),
                reject!(""),
                reject!("xab"),
            ];
        }
    }
}

mod e2e_peg_hex_color {
    peg_marser_e2e! {
        mod inner {
            grammar = "tests/fixtures/peg_hex_color.peg";
            generated = peg_hex_color;
            entry = main;
            corpus = [
                accept!("#ff00aa"),
                accept!("#FF00AA"),
                accept!("#012345"),
                reject!("#gg0000"),
                reject!("#fff"),
                reject!("#1234567"),
                reject!(""),
                reject!("ff00aa"),
            ];
        }
    }
}
