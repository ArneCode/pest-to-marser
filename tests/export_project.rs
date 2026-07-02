//! Integration tests for the downloadable Cargo project scaffold.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use grammar_to_marser::{
    cargo_toml, convert_grammar_source, default_sample_input, gitignore, lib_rs, main_rs, readme,
    suggest_sample_source, ConvertOptions, InputSyntax, MARSER_VERSION,
};
use tempfile::TempDir;

fn workspace_cargo_toml() -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
    )
    .expect("read workspace Cargo.toml")
}

fn materialize_project(
    root: &Path,
    project_name: &str,
    grammar_rs: &str,
    grammar_file: &str,
    grammar_source: &str,
    entry_rule: &str,
    sample_input: &str,
    emit_trace: bool,
) {
    fs::create_dir_all(root.join("src")).expect("create src");
    fs::create_dir_all(root.join("examples")).expect("create examples");
    fs::write(root.join("Cargo.toml"), cargo_toml(project_name, emit_trace)).expect("Cargo.toml");
    fs::write(root.join("README.md"), readme(project_name, entry_rule, emit_trace)).expect("README");
    fs::write(root.join(".gitignore"), gitignore()).expect(".gitignore");
    fs::write(root.join("examples/sample.txt"), sample_input).expect("sample");
    fs::write(root.join(grammar_file), grammar_source).expect("grammar file");
    fs::write(root.join("src/lib.rs"), lib_rs()).expect("lib.rs");
    fs::write(root.join("src/grammar.rs"), grammar_rs).expect("grammar.rs");
    fs::write(root.join("src/main.rs"), main_rs(project_name, emit_trace)).expect("main.rs");
}

fn cargo_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn cargo(root: &Path, args: &[&str]) -> std::process::Output {
    let _guard = cargo_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Command::new("cargo")
        .env("CARGO_TARGET_DIR", root.join("target"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("spawn cargo")
}

fn calc_fixture() -> (String, String) {
    let pest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/calc.pest");
    let grammar_rs_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/generated/calc.rs");
    let pest = fs::read_to_string(&pest_path).expect("read calc.pest");
    let grammar_rs = fs::read_to_string(&grammar_rs_path).expect("read calc.rs");
    (pest, grammar_rs)
}

#[test]
fn marser_version_matches_workspace_cargo_toml() {
    let cargo = workspace_cargo_toml();
    assert!(
        cargo.contains(&format!("marser = \"{MARSER_VERSION}\"")),
        "export_templates::MARSER_VERSION should match workspace Cargo.toml"
    );
}

#[test]
fn export_template_snapshots_match_committed_files() {
    let snapshot_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/export_snapshots");
    let cases = [
        ("cargo_toml", cargo_toml("grammar-parser", false)),
        ("cargo_toml_trace", cargo_toml("grammar-parser", true)),
        ("main_rs", main_rs("grammar-parser", false)),
        ("main_rs_trace", main_rs("grammar-parser", true)),
        ("readme", readme("grammar-parser", "expr", false)),
        ("readme_trace", readme("grammar-parser", "expr", true)),
        ("lib_rs", lib_rs().to_string()),
        ("gitignore", gitignore().to_string()),
    ];

    for (name, actual) in cases {
        let path = snapshot_dir.join(format!("{name}.txt"));
        let expected = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read snapshot {path:?}: {e}"));
        assert_eq!(
            actual, expected,
            "stale tests/export_snapshots/{name}.txt — update snapshots after intentional template changes"
        );
    }
}

#[test]
fn exported_calc_project_builds_and_runs() {
    let (pest, grammar_rs) = calc_fixture();
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    let sample = suggest_sample_source(&pest, InputSyntax::Pest, "expr")
        .expect("suggest")
        .expect("sample");
    assert_eq!(sample, "1");

    materialize_project(
        root,
        "grammar-parser",
        &grammar_rs,
        "grammar.pest",
        &pest,
        "expr",
        &sample,
        false,
    );

    let build = cargo(root, &["build"]);
    assert!(
        build.status.success(),
        "cargo build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );

    let ok_run = cargo(root, &["run", "--", "examples/sample.txt"]);
    assert!(
        ok_run.status.success(),
        "valid input should succeed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&ok_run.stdout),
        String::from_utf8_lossy(&ok_run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&ok_run.stdout).contains("number"),
        "success should print parsed AST:\n{}",
        String::from_utf8_lossy(&ok_run.stdout),
    );
    assert!(
        String::from_utf8_lossy(&ok_run.stdout).contains("\"1\""),
        "parsed AST should include the input value"
    );

    fs::write(root.join("examples/bad.txt"), "1+").expect("write bad input");
    let bad_run = cargo(root, &["run", "--", "examples/bad.txt"]);
    assert!(
        !bad_run.status.success(),
        "invalid input should fail"
    );
    let stderr = String::from_utf8_lossy(&bad_run.stderr);
    assert!(
        !stderr.is_empty(),
        "invalid input should print diagnostics to stderr"
    );
}

#[test]
fn exported_calc_project_with_trace_builds_and_runs() {
    let pest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/calc.pest");
    let pest = fs::read_to_string(&pest_path).expect("read calc.pest");
    let grammar_rs = convert_grammar_source(
        &pest,
        &ConvertOptions {
            entry_rule: "expr".to_string(),
            emit_trace: true,
            ..Default::default()
        },
    )
    .expect("convert calc with trace");

    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    let sample = suggest_sample_source(&pest, InputSyntax::Pest, "expr")
        .expect("suggest")
        .unwrap_or_else(|| default_sample_input().to_string());

    materialize_project(
        root,
        "grammar-parser",
        &grammar_rs,
        "grammar.pest",
        &pest,
        "expr",
        &sample,
        true,
    );

    let build = cargo(root, &["build", "--features", "parser-trace"]);
    assert!(
        build.status.success(),
        "trace build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );

    let ok_run = cargo(
        root,
        &[
            "run",
            "--features",
            "parser-trace",
            "--",
            "examples/sample.txt",
        ],
    );
    assert!(
        ok_run.status.success(),
        "trace run failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&ok_run.stdout),
        String::from_utf8_lossy(&ok_run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&ok_run.stdout).contains("number"),
        "trace run should print parsed AST"
    );

    let trace_path = root.join("trace.json");
    let trace_run = cargo(
        root,
        &[
            "run",
            "--features",
            "parser-trace",
            "--",
            "examples/sample.txt",
            "--trace-file",
            trace_path.to_str().expect("trace path"),
        ],
    );
    assert!(
        trace_run.status.success(),
        "trace-file run failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&trace_run.stdout),
        String::from_utf8_lossy(&trace_run.stderr),
    );
    assert!(
        trace_path.is_file(),
        "trace file should be created at {}",
        trace_path.display()
    );
    let trace_stderr = String::from_utf8_lossy(&trace_run.stderr);
    assert!(
        trace_stderr.contains("trace written to"),
        "should report trace file location, got: {trace_stderr}"
    );
}

#[test]
fn exported_project_with_unsynthesizable_sample_exports_empty_sample_and_readme_warns() {
    let pest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lookahead.pest");
    let pest = fs::read_to_string(&pest_path).expect("read lookahead.pest");
    let grammar_rs = convert_grammar_source(
        &pest,
        &ConvertOptions {
            entry_rule: "main".to_string(),
            ..Default::default()
        },
    )
    .expect("convert lookahead");

    let sample = suggest_sample_source(&pest, InputSyntax::Pest, "main")
        .expect("suggest");
    assert!(
        sample.is_none(),
        "lookahead sample generation should return None"
    );

    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    materialize_project(
        root,
        "grammar-parser",
        &grammar_rs,
        "grammar.pest",
        &pest,
        "main",
        "",
        false,
    );

    let readme = fs::read_to_string(root.join("README.md")).expect("read README");
    assert!(
        readme.contains("best-effort basis") && readme.contains("may be empty"),
        "README should warn about missing samples"
    );
    let exported_sample =
        fs::read_to_string(root.join("examples/sample.txt")).expect("read sample");
    assert_eq!(exported_sample, "", "exported sample should be empty");

    let build = cargo(root, &["build"]);
    assert!(
        build.status.success(),
        "cargo build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );

    let run = cargo(root, &["run", "--", "examples/sample.txt"]);
    assert!(
        !run.status.success(),
        "empty sample should not parse for lookahead fixture"
    );
}

#[test]
fn exported_project_preserves_trailing_newline_in_sample_input() {
    let pest_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/trailing_newline.pest");
    let pest = fs::read_to_string(&pest_path).expect("read trailing_newline.pest");
    let grammar_rs = convert_grammar_source(
        &pest,
        &ConvertOptions {
            entry_rule: "main".to_string(),
            ..Default::default()
        },
    )
    .expect("convert trailing_newline");

    let sample = suggest_sample_source(&pest, InputSyntax::Pest, "main")
        .expect("suggest")
        .expect("sample");
    assert_eq!(sample, "a\n", "sample should include trailing newline");

    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    materialize_project(
        root,
        "grammar-parser",
        &grammar_rs,
        "grammar.pest",
        &pest,
        "main",
        &sample,
        false,
    );

    let exported_sample =
        fs::read_to_string(root.join("examples/sample.txt")).expect("read sample");
    assert_eq!(
        exported_sample, sample,
        "export should preserve sample text exactly"
    );

    let build = cargo(root, &["build"]);
    assert!(
        build.status.success(),
        "cargo build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );

    let run = cargo(root, &["run", "--", "examples/sample.txt"]);
    assert!(
        run.status.success(),
        "trailing-newline sample should parse:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
}
