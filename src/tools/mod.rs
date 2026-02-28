pub mod calculator;
pub mod counter;
pub mod time;

pub use calculator::Multiply;
pub use counter::{CounterState, GetCounter, IncrementCounter};
pub use time::GetCurrentTime;