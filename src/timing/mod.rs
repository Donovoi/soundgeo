//! Acoustic passage-timing research. Qualification is separate from Doppler.
mod assessment;
mod assignment;
mod audio;
pub mod likelihood;
mod reference;
mod search;

pub use assessment::{Assessment, LocationRegion, Screen, assess};
pub use audio::{Peak, detect};
pub use search::{Candidate, LocationScore, Options, Pair, SearchResult, locate};
