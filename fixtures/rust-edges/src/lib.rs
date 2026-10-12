//! A lamp workshop: lamps, wicks, shelves and the polish that keeps them bright.

pub mod lamp;
pub mod shelf;
mod glow;

pub mod tools {
    pub mod polish;

    pub fn buff(level: u32) -> u32 {
        level + 1
    }
}

pub use lamp::wick::trim;
pub use shelf::Shelf;

/// Something that gives off light.
pub trait Shine {
    fn beam(&self) -> u32;
}

pub fn light(level: u32) -> u32 {
    let lamp = lamp::Lamp::new(level);
    lamp.beam() + tools::buff(level)
}

pub fn dim(level: u32) -> u32 {
    let light = |x: u32| x / 2;
    std::cmp::max(light(level), 1)
}

pub fn fresh<T: Default>() -> T {
    T::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_adds_the_buff() {
        assert_eq!(light(1), tools::buff(1) + 1);
        let trimmed = vec![trim(4)];
        assert!(matches!(shelf::count(&Shelf::default()), 0));
        assert_eq!(trimmed.len(), 1);
    }
}
