//! Regenerate committed export template snapshots.
//! Run: cargo run --features dev-tools --bin update-export-snapshots

use std::fs;
use std::path::PathBuf;

use grammar_to_marser::{cargo_toml, gitignore, lib_rs, main_rs, readme};

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = root.join("tests/export_snapshots");
    fs::create_dir_all(&out_dir).expect("create export_snapshots");

    let snapshots = [
        ("cargo_toml", cargo_toml("grammar-parser", false)),
        ("cargo_toml_trace", cargo_toml("grammar-parser", true)),
        ("main_rs", main_rs("grammar-parser", false)),
        ("main_rs_trace", main_rs("grammar-parser", true)),
        ("readme", readme("grammar-parser", "expr", false)),
        ("readme_trace", readme("grammar-parser", "expr", true)),
        ("lib_rs", lib_rs().to_string()),
        ("gitignore", gitignore().to_string()),
    ];

    for (name, content) in snapshots {
        let path = out_dir.join(format!("{name}.txt"));
        fs::write(&path, content).expect("write snapshot");
        eprintln!("wrote {}", path.display());
    }
}
