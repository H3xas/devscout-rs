mod common;

#[test]
fn light_reaches_the_polish() {
    assert_eq!(rust_edges::light(1), common::expected());
    let shined = rust_edges::tools::polish::shine_up(1);
    assert_eq!(shined, 2);
}
