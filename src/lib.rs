#![doc = include_str!("../README.md")]

/// Collection of coin selection algorithms: Branch and Bound (BnB), CoinGrinder, First-In-First-Out (FIFO), and Lowest Larger
pub mod algorithms;
/// Wrapper API that runs all coin selection algorithms in parallel and returns the result with lowest waste
pub mod selectcoin;
/// Core types and structs used throughout the library including OutputGroup and CoinSelectionOpt
pub mod types;
/// Helper functions with tests for fee calculation, weight computation, and waste metrics
pub mod utils;
