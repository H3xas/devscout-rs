pub struct Lamp;

pub fn dust(n: u32) -> u32 {
    let lamp = Lamp;
    lamp.glow();
    n
}

impl Lamp {
    pub fn glow(&self) {}
}
