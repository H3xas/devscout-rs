use std::fmt;

use crate::lamp::Lamp;
use crate::Shine;

impl Shine for Lamp {
    fn beam(&self) -> u32 {
        self.level
    }
}

impl fmt::Display for Lamp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", Lamp::brightness(self))
    }
}

impl Lamp {
    pub fn brightness(&self) -> u32 {
        Self::scale(self.level)
    }

    fn scale(level: u32) -> u32 {
        level * 2
    }
}
