mod dust;

#[derive(Default)]
pub struct Shelf {
    pub lamps: Vec<crate::lamp::Lamp>,
}

pub fn count(shelf: &Shelf) -> usize {
    dust::sweep(shelf.lamps.len())
}
