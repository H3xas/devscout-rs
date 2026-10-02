//! A kite workshop: kites, reels and the line that ties them.

#[path = "real.rs"]
pub mod alias;

#[path = "deep/hub.rs"]
pub mod hub;

pub mod checks;
pub mod reel;

mod bay {
    #[path = "dock.rs"]
    pub mod pier;
}

/// A kite on the bench.
pub struct Kite;

impl Kite {
    pub fn new() -> Kite {
        Kite
    }
}

pub fn fly() {}

pub fn launch() {
    alias::f();
    hub::center();
    hub::spoke::spin();
    bay::pier::moor();
    reel::wind_all();
}
