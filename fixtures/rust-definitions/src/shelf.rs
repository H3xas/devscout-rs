#![allow(dead_code)]
//! Shelves, the widgets that sit on them, and how they are polished.

use std::collections::BTreeMap as ShelfMap;
use std::fmt;

/// A widget that sits on a shelf.
// Widths are in millimetres.
#[derive(Debug, Clone)]
pub struct Widget {
    spine_width: u32,
}

/// The colours a widget can take.
pub enum Tone {
    Crimson,
    Slate,
}

/// Two views of one shelf word.
#[repr(C)]
pub union Bits {
    whole: u32,
    halves: [u16; 2],
}

// Drawing helpers.
/// Anything that can be drawn on a shelf.
pub trait Render {
    /// The surface a drawing lands on.
    type Canvas;
    /// How many layers a drawing has.
    const LAYERS: u32;
    /// Draws onto the canvas.
    fn draw(&self, canvas: &mut Self::Canvas);
    /// Draws the outline only.
    fn outline(&self) -> u32 {
        Self::LAYERS
    }
}

/// A shade of a tone.
pub type Shade = u8;

/// Polishes every widget on the shelf.
pub fn polish(widgets: &mut [Widget]) {
    fn tucked_helper(w: &mut Widget) {
        w.spine_width += 1;
    }
    for w in widgets {
        tucked_helper(w);
    }
}

/// How deep a shelf is.
pub const SHELF_DEPTH: u32 = 300;

/// The label printed on every shelf; the 𝔸 grade is the widest.
pub static SHELF_LABEL: &str = "shelf";

/// Builds a stack of widgets.
macro_rules! stack_of {
    ($n:expr) => {{
        fn made_by_macro() {}
        $n
    }};
}

mod drawers;

impl Widget {
    /// The widest spine a widget may have.
    pub const MAX_SPINE: u32 = 40;

    /// A new widget with a narrow spine.
    pub fn new() -> Self {
        Widget { spine_width: 1 }
    }

    /// The spine width.
    pub fn spine(&self) -> u32 {
        self.spine_width
    }
}

impl Render for Widget {
    type Canvas = Vec<u8>;
    const LAYERS: u32 = 2;

    fn draw(&self, canvas: &mut Self::Canvas) {
        canvas.push(0);
    }
}

/// A gadget shown two ways.
pub struct Gadget;

impl fmt::Display for Gadget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "gadget")
    }
}

impl fmt::Debug for Gadget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Gadget")
    }
}

#[cfg(unix)]
pub fn home_shelf() -> &'static str {
    "/shelf"
}

#[cfg(not(unix))]
pub fn home_shelf() -> &'static str {
    "C:\\shelf"
}

pub fn r#match(a: u32, b: u32) -> bool {
    a == b
}

const _: () = ();

/// A plank stiffened at compile time.
const PLANK: u32 = {
    fn hidden_in_const() -> u32 {
        3
    }
    hidden_in_const()
};

thread_local! {
    static TUCKED_COUNTER: u32 = 0;
}
