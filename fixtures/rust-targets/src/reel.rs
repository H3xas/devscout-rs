#[path = "../gear/cog.rs"]
pub mod cog;

mod spool {
    #[path = "thread.rs"]
    pub mod line;
}

pub fn wind_all() {
    cog::turn();
    spool::line::wind();
}
