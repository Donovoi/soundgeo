//! Acoustic passage-timing research. Qualification is separate from Doppler.
mod assignment;
mod audio;
pub mod likelihood;
mod reference;
mod search;

pub use audio::{Peak, detect};
pub use search::{Candidate, Options, Pair, SearchResult, locate};
