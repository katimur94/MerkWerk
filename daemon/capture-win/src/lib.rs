//! Windows-specific capture module for MerkWerk.

#[cfg(windows)]
/// Placeholder capture-win module for Windows.
pub fn placeholder() {}

#[cfg(not(windows))]
/// Empty module on non-Windows platforms.
pub fn placeholder() {}
