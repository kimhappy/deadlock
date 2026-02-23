#![doc = include_str!("../README.md")]

mod inner;
mod util;

pub mod slotheap;
pub mod slotmap;

pub use slotheap::*;
pub use slotmap::*;
