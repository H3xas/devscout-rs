pub mod wick;

pub struct Lamp {
    pub level: u32,
}

impl Lamp {
    pub fn new(level: u32) -> Lamp {
        Lamp {
            level: wick::trim(level),
        }
    }
}

pub fn relight() -> u32 {
    wick::light()
}
