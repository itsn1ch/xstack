pub mod budget;
pub mod git;
pub mod probe;
pub mod queue;
pub mod run;
pub mod scheduler;
pub mod split;
pub mod spool;
pub mod state;

pub type Result<T> = std::result::Result<T, String>;
