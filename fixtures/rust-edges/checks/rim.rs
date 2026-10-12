mod helpers;

use rust_edges::shelf::count;

#[test]
fn an_empty_shelf_counts_nothing() {
    assert_eq!(count(&helpers::empty()), 0);
}

#[test]
fn trimming_caps_the_wick() {
    assert_eq!(rust_edges::trim(20), 10);
}
