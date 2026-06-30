use grammar_to_marser::{convert_source, convert_grammar_source, ConvertOptions, InputSyntax};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize)]
struct Manifest {
    fixture: Vec<Fixture>,
    #[serde(default)]
    peg_fixture: Vec<PegFixture>,
}

#[derive(Deserialize)]
struct Fixture {
    pest: String,
    entry: String,
    stem: String,
}

#[derive(Deserialize)]
struct PegFixture {
    peg: String,
    entry: String,
    stem: String,
}

fn main() {
    let manifest_src = fs::read_to_string("tests/fixtures.toml").expect("read tests/fixtures.toml");
    let manifest: Manifest = toml::from_str(&manifest_src).expect("parse tests/fixtures.toml");

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let generated_dir = root.join("tests/generated");
    fs::create_dir_all(&generated_dir).expect("create tests/generated");

    for fixture in &manifest.fixture {
        let pest_path = root.join("tests/fixtures").join(&fixture.pest);
        let source = fs::read_to_string(&pest_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", pest_path.display()));
        let code = convert_grammar_source(
            &source,
            &ConvertOptions {
                entry_rule: fixture.entry.clone(),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("convert {}: {e:?}", fixture.pest));

        let out_path = generated_dir.join(format!("{}.rs", fixture.stem));
        fs::write(&out_path, code).expect("write generated snapshot");
        println!("updated {}", out_path.display());
    }

    for fixture in &manifest.peg_fixture {
        let peg_path = root.join("tests/fixtures").join(&fixture.peg);
        let source = fs::read_to_string(&peg_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", peg_path.display()));
        let code = convert_source(
            &source,
            InputSyntax::Peg,
            &ConvertOptions {
                entry_rule: fixture.entry.clone(),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("convert {}: {e:?}", fixture.peg));

        let out_path = generated_dir.join(format!("{}.rs", fixture.stem));
        fs::write(&out_path, code).expect("write generated snapshot");
        println!("updated {}", out_path.display());
    }
}
