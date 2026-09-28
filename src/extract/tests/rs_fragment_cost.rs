use super::*;

#[test]
fn generated_item_cost_row() {
    let Ok(path) = std::env::var("RESOLVER_COST_ITEM") else {
        return;
    };
    let source = std::fs::read_to_string(&path).expect("read generated source");
    crate::extract::rs_fragment::reset_item_cost();
    let extracted = extract_rs_file(&source);
    let cost = crate::extract::rs_fragment::item_cost();
    let row = serde_json::json!({
        "family": "item", "source": std::path::Path::new(&path).file_name().unwrap().to_string_lossy(),
        "input_bytes": source.len(), "items": cost.items,
        "parent_calls": cost.parent_calls, "awork": cost.ancestry_work,
        "path_bytes": cost.path_bytes,
        "fragment_bytes": serde_json::to_vec(&extracted.fragment).unwrap().len(),
    });
    println!("COST_ROW={row}");
}
