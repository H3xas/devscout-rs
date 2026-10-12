pub mod spoke;

pub fn center() {}

#[test]
fn hub_is_compiled() {
    spoke::spin();
}
