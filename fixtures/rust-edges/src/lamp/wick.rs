pub fn trim(length: u32) -> u32 {
    length.min(10)
}

pub fn light() -> u32 {
    super::super::light(1)
}
