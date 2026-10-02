mod part;

fn helper() {}

#[test]
fn flight_reaches_the_library() {
    rust_targets::fly();
    let _kite = rust_targets::Kite::new();
    part::check();
}
