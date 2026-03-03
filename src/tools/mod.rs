pub mod calculator;
pub mod counter;
pub mod file_reader;
pub mod time;
pub mod weather;

pub use calculator::Multiply;
pub use counter::{CounterState, GetCounter, IncrementCounter};
pub use file_reader::ReadFile;
pub use time::GetCurrentTime;
pub use weather::GetWeather;