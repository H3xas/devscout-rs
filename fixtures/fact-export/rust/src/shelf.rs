/// How wide the shelf is.
pub fn width() -> u32 {
    600
}

/// The shelf's label, built from its width.
pub fn label() -> String {
    format!("{}", width())
}
