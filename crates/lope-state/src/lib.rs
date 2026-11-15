//! Lope-owned state shared with Python Lope (S1): the run registry
//! (`jobs.py`), process identity, and the run lock (`runlock.py`).

pub mod process;
pub mod registry;
pub mod runlock;
