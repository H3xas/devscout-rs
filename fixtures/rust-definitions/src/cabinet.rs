// Cabinets and the fittings inside them.

/// Everything that goes inside a cabinet.
pub mod cabinet {
    //! Doors, hinges and the rest of a cabinet.
    /// A hinge on a cabinet door.
    pub struct Hinge {
        turns: u32,
    }

    /// Which way a door opens.
    pub enum Swing {
        Left,
        Right,
    }

    /// A latch read either way.
    pub union Latch {
        closed: u8,
        open: i8,
    }

    /// Something that can be opened.
    pub trait Open {
        /// Opens it.
        fn open(&mut self);
    }

    /// A screw size.
    pub type Screw = u16;

    /// Puts a cabinet together.
    pub fn assemble() -> Hinge {
        Hinge { turns: 0 }
    }

    /// How wide a cabinet is.
    pub const CABINET_WIDTH: u32 = 600;

    /// The cabinet's maker.
    pub static CABINET_MAKER: &str = "maker";

    /// Repeats a fitting.
    macro_rules! twice {
        ($e:expr) => {
            ($e, $e)
        };
    }

    mod fittings;

    impl Hinge {
        /// Oils the hinge.
        pub fn oil(&mut self) {
            self.turns += 1;
        }
    }

    impl Open for Hinge {
        fn open(&mut self) {
            self.turns += 1;
        }
    }
}

extern "C" {
    /// How many shelves the host reports.
    fn shelf_count() -> i32;
    /// The host's last shelf error.
    static SHELF_ERRNO: i32;
    /// An opaque handle to a host shelf.
    type ShelfHandle;
}
