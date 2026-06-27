use pest_to_marser::{convert_pest_source, ConvertOptions};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn convert(pest_source: &str, entry_rule: &str) -> Result<String, String> {
    let options = ConvertOptions {
        entry_rule: entry_rule.to_string(),
        function_name: "grammar".to_string(),
    };

    convert_pest_source(pest_source, &options).map_err(|errors| {
        errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })
}
