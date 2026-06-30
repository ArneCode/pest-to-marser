use std::{env, fs, process};

use grammar_to_marser::{convert_source, ConvertOptions, InputSyntax};

fn usage() -> ! {
    eprintln!(
        "usage: grammar-to-marser <grammar-file> [entry_rule] [--syntax pest|peg] [--output <path>] [--trace]"
    );
    process::exit(1);
}

fn main() {
    let mut args = env::args().skip(1);
    let path = args.next().unwrap_or_else(|| usage());
    let mut entry_rule = String::new();
    let mut output_path = None;
    let mut emit_trace = false;
    let mut syntax = InputSyntax::Pest;
    let mut arg_iter = args;
    while let Some(arg) = arg_iter.next() {
        if arg == "--output" {
            output_path = Some(arg_iter.next().unwrap_or_else(|| usage()));
        } else if arg == "--trace" {
            emit_trace = true;
        } else if arg == "--syntax" {
            let value = arg_iter.next().unwrap_or_else(|| usage());
            syntax = InputSyntax::parse(&value).unwrap_or_else(|| {
                eprintln!("unknown syntax: {value} (expected pest or peg)");
                usage();
            });
        } else if entry_rule.is_empty() {
            entry_rule = arg;
        } else {
            eprintln!("unknown argument: {arg}");
            usage();
        }
    }

    let src = fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("failed to read {path}: {e}");
        process::exit(1);
    });

    match convert_source(
        &src,
        syntax,
        &ConvertOptions {
            entry_rule,
            function_name: "grammar".to_string(),
            emit_trace,
            ..Default::default()
        },
    ) {
        Ok(code) => {
            if let Some(out) = output_path {
                fs::write(&out, &code).unwrap_or_else(|e| {
                    eprintln!("failed to write {out}: {e}");
                    process::exit(1);
                });
                println!("wrote {out}");
            } else {
                print!("{code}");
            }
        }
        Err(errors) => {
            for error in errors {
                eprintln!("error: {error}");
            }
            process::exit(1);
        }
    }
}
