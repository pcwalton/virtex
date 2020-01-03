// virtex/src/lib.rs

#[macro_use]
extern crate log;

pub mod manager;
pub mod renderer_advanced;
pub mod renderer_simple;
pub mod svg;
pub mod texture;

mod stack;

#[cfg(test)]
mod tests;
