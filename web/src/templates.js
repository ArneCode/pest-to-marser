const MARSER_VERSION = "0.2.2";

const MARSER_DEP = `marser = { version = "${MARSER_VERSION}", features = ["annotate-snippets"] }`;

export function cargoToml(projectName, emitTrace = false) {
  if (!emitTrace) {
    return `[package]
name = "${projectName}"
version = "0.1.0"
edition = "2024"

[dependencies]
${MARSER_DEP}
`;
  }

  return `[package]
name = "${projectName}"
version = "0.1.0"
edition = "2024"

[features]
default = []
parser-trace = ["marser/parser-trace"]

[dependencies]
${MARSER_DEP}
`;
}

export function mainRs(emitTrace = false) {
  if (!emitTrace) {
    return `use marser::parser::Parser;

mod grammar;

fn main() {
    let input = std::io::read_to_string(std::io::stdin()).expect("failed to read stdin");
    match grammar::grammar().parse_str(&input) {
        Ok(_) => println!("OK"),
        Err(err) => {
            err.eprint("<stdin>", &input);
            std::process::exit(1);
        }
    }
}
`;
  }

  return `use std::{env, fs, process};

use marser::parser::Parser;
#[cfg(feature = "parser-trace")]
use marser::trace::TraceFormat;

mod grammar;

fn usage(program: &str) -> ! {
    eprintln!(
        "Usage: {program} <input-file> [--trace-file <path>]"
    );
    process::exit(2);
}

fn main() {
    let program = env::args().next().unwrap_or_else(|| "pest-parser".to_string());
    let mut args = env::args().skip(1);
    let path = args.next().unwrap_or_else(|| usage(&program));
    let mut trace_file = None;

    for arg in args {
        if arg == "--trace-file" {
            trace_file = Some(
                args.next()
                    .unwrap_or_else(|| usage(&program)),
            );
        } else {
            usage(&program);
        }
    }

    let input = fs::read_to_string(&path).unwrap_or_else(|err| {
        eprintln!("failed to read {path}: {err}");
        process::exit(1);
    });

    #[cfg(feature = "parser-trace")]
    {
        let parser = grammar::grammar();
        if let Some(trace_path) = trace_file {
            match parser.parse_str_with_trace_to_file(
                &input,
                &trace_path,
                TraceFormat::Json,
            ) {
                Ok(_) => {
                    eprintln!("trace written to {trace_path}");
                    println!("OK");
                }
                Err(marser::ParseWithTraceToFileError::Parse(err)) => {
                    err.eprint(&path, &input);
                    process::exit(1);
                }
                Err(marser::ParseWithTraceToFileError::Io(err)) => {
                    eprintln!("failed to write trace file '{trace_path}': {err}");
                    process::exit(1);
                }
            }
        } else {
            match parser.parse_str_with_trace(&input) {
                Ok((_, _errors, _trace)) => println!("OK"),
                Err(err) => {
                    err.eprint(&path, &input);
                    process::exit(1);
                }
            }
        }
    }

    #[cfg(not(feature = "parser-trace"))]
    {
        let _ = trace_file;
        eprintln!("rebuild with --features parser-trace to collect parser traces");
        match grammar::grammar().parse_str(&input) {
            Ok(_) => println!("OK"),
            Err(err) => {
                err.eprint(&path, &input);
                process::exit(1);
            }
        }
    }
}
`;
}

export function readme(projectName, entryRule, emitTrace = false) {
  const entry = entryRule.trim() || "(last rule in grammar)";
  const buildSection = emitTrace
    ? `## Build and run

\`\`\`sh
cargo build --features parser-trace
cargo run --features parser-trace -- sample.txt
\`\`\`

Pass an input file path. The parser prints \`OK\` on success.`
    : `## Build and run

\`\`\`sh
cargo build
echo 'your input' | cargo run
\`\`\`

The parser accepts input on stdin and prints \`OK\` on success.`;

  const tracingSection = emitTrace
    ? `
## Tracing and debugging

This project was generated with \`.trace()\` markers in \`src/grammar.rs\`. To record a parse trace and step through it in the [marser trace viewer](https://crates.io/crates/marser-trace-viewer):

\`\`\`sh
# install the viewer once
cargo install marser-trace-viewer

# parse a file and write a trace
cargo run --features parser-trace -- sample.txt --trace-file trace.json

# open the trace (use the same input file for span preview)
marser-trace-viewer --trace trace.json --source sample.txt
\`\`\`

Tracing adds runtime overhead; use \`parser-trace\` for debugging rather than production builds.
`
    : "";

  const nextStepsSection = `
## Next steps

The generated parser only checks that input matches your grammar (\`capture!(… => ())\`). Typical follow-ups:

1. **Build an AST** — \`src/grammar.rs\` already uses \`bind!\` for rule references. Change each \`capture!\` output from \`()\` to a real type and assemble values from those binds. See [Capture and Binds](https://docs.rs/marser/latest/marser/guide/capture_and_binds/index.html) and the [worked JSON example](https://docs.rs/marser/latest/marser/guide/worked_json_example/index.html).
2. **Improve diagnostics** — use \`.with_label(...)\` on rules, \`add_error_info\`, and \`annotate-snippets\` output (already enabled in \`Cargo.toml\`). See [Errors and Recovery](https://docs.rs/marser/latest/marser/guide/errors_and_recovery/index.html).
3. **Recover from errors** — return partial results with \`recover_with\`, inline hints with \`try_insert_if_missing\` / \`unwanted\`, and commits with \`commit_on\` where backtracking should stop. Same guide: [Errors and Recovery](https://docs.rs/marser/latest/marser/guide/errors_and_recovery/index.html).
4. **Refine the grammar** — whitespace, lists, and recursion recipes in [Common patterns](https://docs.rs/marser/latest/marser/guide/common_patterns/index.html).${emitTrace ? "" : "\n5. **Debug parsing** — re-generate with **Trace** enabled in pest-to-marser, or add \`.trace()\` markers by hand. See [Tracing and Debugging](https://docs.rs/marser/latest/marser/guide/tracing_and_debugging/index.html)."}

Full guide index: [marser guide](https://docs.rs/marser/latest/marser/guide/index.html).
`;

  return `# ${projectName}

Generated by [pest-to-marser](https://github.com/ArneCode/pest-to-marser).

## Entry rule

\`${entry}\`

${buildSection}
${tracingSection}${nextStepsSection}`;
}
