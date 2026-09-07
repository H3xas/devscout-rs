use super::blocks::{compact_block, ref_kind_block, rle};
use super::*;
use crate::graph::HeuristicTier;
use crate::query;
use crate::query::{
    AmbiguousRow, AmbiguousTables, DefSite, ImpactModel, ImpactRow, ImportRow, InboundRow,
    InboundTables, OutboundRow, OutboundTables, RefsModel, SeedKind, Table, TestRow, TestsModel,
    Why,
};

mod blocks;
mod coverage;
mod impact;
mod markers;
mod refs;

fn table<R>(rows: Vec<R>, dropped: usize) -> Table<R> {
    Table {
        total: rows.len() + dropped,
        dropped,
        rows,
    }
}
