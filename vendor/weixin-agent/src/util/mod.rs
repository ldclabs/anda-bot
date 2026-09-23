//! Utility helpers: log redaction and ID generation.

pub mod random;
pub mod redact;

/// Current time in milliseconds since UNIX epoch.
#[allow(clippy::cast_possible_truncation)] // u128 → u64: won't overflow until year 584942417
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
