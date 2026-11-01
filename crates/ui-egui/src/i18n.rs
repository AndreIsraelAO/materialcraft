//! Interface text. English only for now; every user-facing string goes through [`tr`] (or is
//! marked with [`n_`] where it is stored), so translations can be added without touching callers.

/// Mark a string for translation where it is stored; translate with [`tr`] where it is shown.
pub const fn n_(text: &'static str) -> &'static str {
    text
}

/// The text in the interface language.
pub fn tr(text: &str) -> &str {
    text
}
