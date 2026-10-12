pub mod shelf;

pub fn polish(n: u32) -> u32 {
    shelf::dust(n) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polish_adds_one() {
        assert_eq!(polish(1), shelf::dust(dust_twice(1)) + 1);
    }

    fn dust_twice(n: u32) -> u32 {
        shelf::dust(shelf::dust(n))
    }
}
