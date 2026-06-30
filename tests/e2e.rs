/// Corpus case labels for Pest vs marser equivalence tests.
#[macro_export]
macro_rules! accept {
    ($input:expr) => {
        ($input, true)
    };
}

#[macro_export]
macro_rules! reject {
    ($input:expr) => {
        ($input, false)
    };
}

/// Shared macro for Pest vs marser accept/reject equivalence tests.
///
/// Generated parser snapshots in `tests/generated/` are kept in sync with the
/// converter via drift tests (`committed_generated_snapshots_match_converter`).
#[macro_export]
macro_rules! pest_marser_e2e {
    (
        mod $mod_name:ident {
            grammar = $grammar_path:literal;
            pest = $pest_name:ident;
            generated = $generated_stem:ident;
            entry = $entry_rule:ident;
            corpus = [$($case:expr),+ $(,)?];
        }
    ) => {
        $crate::pest_marser_e2e! {
            @body $mod_name, $grammar_path, $pest_name, $generated_stem, $entry_rule, [$($case),+]
        }
    };
    (
        mod $mod_name:ident {
            grammar = $grammar_path:literal;
            pest = $pest_name:ident;
            generated = $generated_stem:ident;
            entry = $entry_rule:ident;
            corpus = [$($case:expr),+ $(,)?];
            fuzz;
        }
    ) => {
        $crate::pest_marser_e2e! {
            @body $mod_name, $grammar_path, $pest_name, $generated_stem, $entry_rule, [$($case),+]
        }

        #[test]
        fn fuzz_matches_pest_oracle() {
            $crate::pest_marser_e2e! {
                @fuzz $mod_name, $pest_name, $generated_stem, $entry_rule
            }
        }
    };
    (
        @fuzz $mod_name:ident, $pest_name:ident, $generated_stem:ident, $entry_rule:ident
    ) => {
        use $mod_name::$pest_name;
        use marser::parser::Parser as MarserParser;
        use pest::Parser;

        fn pest_accepts_full_input(input: &str) -> bool {
            match $pest_name::parse($mod_name::Rule::$entry_rule, input) {
                Ok(mut pairs) => pairs
                    .next()
                    .map(|pair| pair.as_span().end() == input.len())
                    .unwrap_or(false),
                Err(_) => false,
            }
        }

        use proptest::prelude::*;

        proptest!(|(input in r"\PC{0,32}")| {
            let pest_ok = pest_accepts_full_input(&input);
            let marser_ok = $mod_name::generated::grammar().parse_str(&input).is_ok();
            prop_assert_eq!(pest_ok, marser_ok, "fuzz mismatch on {:?}", input);
        });
    };
    (
        @body $mod_name:ident, $grammar_path:literal, $pest_name:ident, $generated_stem:ident,
        $entry_rule:ident, [$($case:expr),+]
    ) => {
        mod $mod_name {
            use marser::parser::Parser as MarserParser;
            use pest::Parser;
            use pest_derive::Parser as PestDerive;

            #[derive(PestDerive)]
            #[grammar = $grammar_path]
            pub(super) struct $pest_name;

            pub(super) mod generated {
                include!(concat!("generated/", stringify!($generated_stem), ".rs"));
            }

            fn pest_accepts_full_input(input: &str) -> bool {
                match $pest_name::parse(Rule::$entry_rule, input) {
                    Ok(mut pairs) => pairs
                        .next()
                        .map(|pair| pair.as_span().end() == input.len())
                        .unwrap_or(false),
                    Err(_) => false,
                }
            }

            #[test]
            fn accept_reject_corpora_match() {
                let cases: &[(&str, bool)] = &[$($case),+];
                for &(input, expected) in cases {
                    let pest_ok = pest_accepts_full_input(input);
                    let marser_ok = generated::grammar().parse_str(input).is_ok();

                    assert_eq!(
                        pest_ok, expected,
                        "Pest oracle disagrees with corpus label on input {input:?}: \
                         pest accepts full input = {pest_ok}, expected = {expected}"
                    );
                    assert_eq!(
                        marser_ok, expected,
                        "marser parser disagrees with corpus label on input {input:?}: \
                         marser accepts = {marser_ok}, expected = {expected}"
                    );
                    assert_eq!(
                        pest_ok, marser_ok,
                        "Pest/marser mismatch on input {input:?}: \
                         pest accepts full input = {pest_ok}, marser accepts = {marser_ok}, \
                         expected = {expected}"
                    );
                }
            }
        }
    };
}

/// Corpus-driven accept/reject tests for PEG-generated parsers (no Pest oracle).
#[macro_export]
macro_rules! peg_marser_e2e {
    (
        mod $mod_name:ident {
            grammar = $grammar_path:literal;
            generated = $generated_stem:ident;
            entry = $entry_rule:ident;
            corpus = [$($case:expr),+ $(,)?];
        }
    ) => {
        $crate::peg_marser_e2e! {
            @body $mod_name, $grammar_path, $generated_stem, $entry_rule, [$($case),+]
        }
    };
    (
        @body $mod_name:ident, $grammar_path:literal, $generated_stem:ident,
        $entry_rule:ident, [$($case:expr),+]
    ) => {
        mod $mod_name {
            use marser::parser::Parser as MarserParser;

            pub(super) mod generated {
                include!(concat!("generated/", stringify!($generated_stem), ".rs"));
            }

            #[test]
            fn accept_reject_corpora_match() {
                let cases: &[(&str, bool)] = &[$($case),+];
                for &(input, expected) in cases {
                    let marser_ok = generated::grammar().parse_str(input).is_ok();
                    assert_eq!(
                        marser_ok, expected,
                        "marser parser disagrees with corpus label on input {input:?}: \
                         marser accepts = {marser_ok}, expected = {expected}"
                    );
                }
            }
        }
    };
}
