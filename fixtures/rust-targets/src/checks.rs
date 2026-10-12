use crate::fly;

#[cfg_attr(test, test)]
pub fn flies_under_test() {
    fly();
}

#[cfg_attr(any(test, doc), test)]
pub fn flies_under_any() {
    fly();
}

#[cfg_attr(test, cfg_attr(all(), test))]
pub fn flies_nested() {
    fly();
}

#[cfg_attr(test, test, ignore)]
pub fn flies_ignored() {
    fly();
}

#[cfg_attr(test, allow(dead_code))]
pub fn not_a_test() {
    fly();
}
