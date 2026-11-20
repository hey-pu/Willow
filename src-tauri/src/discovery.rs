use std::path::PathBuf;

/// A stable source identity; matching multiple probes does not create duplicate rows.
pub struct Source {
    pub agent: &'static str,
    pub label: &'static str,
    pub detected: bool,
    pub dirs: Vec<PathBuf>,
    pub variants: Vec<&'static str>,
}

#[cfg(not(windows))]
pub fn scan() -> Vec<Source> {
    Vec::new()
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::scan;
