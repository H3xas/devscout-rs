pub mod shelf;

/// Something that can be drawn.
pub trait Render {
    /// Draws it.
    fn draw(&self) -> u32;
}

/// A stack of layers.
pub struct Widget {
    layers: u32,
}

impl Render for Widget {
    fn draw(&self) -> u32 {
        self.layers
    }
}

/// Draws a widget through the trait.
pub fn outline(widget: &Widget) -> u32 {
    widget.draw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_reads_the_layers() {
        let widget = Widget { layers: 2 };
        assert_eq!(outline(&widget), 2);
    }
}
