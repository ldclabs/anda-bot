use anda_core::BoxError;
use jiff::{SignedDuration, Timestamp, civil::DateTime, fmt::strtime::BrokenDownTime, tz::Offset};

/// The RFC 3339 wire format used by cron, MCP and the evaluation host. Jiff's
/// general timestamp parser also accepts abbreviated ISO dates and zone annotations.
pub fn parse_rfc3339(value: &str) -> Result<Timestamp, BoxError> {
    let invalid = "expected an RFC 3339 timestamp";
    let (whole, tail) = value.split_at_checked(19).ok_or(invalid)?;
    if !whole.bytes().enumerate().all(|(index, byte)| match index {
        4 | 7 => byte == b'-',
        10 => matches!(byte, b'T' | b't' | b' '),
        13 | 16 => byte == b':',
        _ => byte.is_ascii_digit(),
    }) {
        return Err(invalid.into());
    }
    let (fraction, zone) = tail.split_at(tail.find(['+', '-', 'Z', 'z']).ok_or(invalid)?);
    let offset = match zone.as_bytes() {
        [b'Z' | b'z'] => Offset::UTC,
        [b'+' | b'-', h1, h2, b':', _, _] if [*h1, *h2] <= *b"23" => {
            BrokenDownTime::parse("%:z", zone)?
                .offset()
                .ok_or(invalid)?
        }
        _ => return Err(invalid.into()),
    };
    if !fraction.is_empty()
        && !fraction
            .strip_prefix('.')
            .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(invalid.into());
    }
    // RFC 3339 allows arbitrary fractional precision. Keep up to nanoseconds.
    let civil: DateTime = value[..19 + fraction.len().min(10)].parse()?;
    let timestamp = offset.to_timestamp(civil)?;
    // Jiff maps a leap second to :59. Existing wire timestamps count it as the next second.
    if whole.ends_with(":60") {
        return Ok(timestamp.checked_add(SignedDuration::from_secs(1))?);
    }
    Ok(timestamp)
}

pub fn rfc3339_to_unix_ms(value: &str) -> Result<u64, BoxError> {
    let timestamp = parse_rfc3339(value)?;
    if timestamp < Timestamp::UNIX_EPOCH {
        return Err("timestamp before unix epoch".into());
    }
    Ok(timestamp.as_millisecond() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_timestamps_require_full_dates_times_and_offsets() {
        for spelling in [
            "1970-01-01T00:00:00.123Z",
            "1970-01-01t00:00:00.12345678901z",
            "1970-01-01 08:00:00.123+08:00",
        ] {
            assert_eq!(rfc3339_to_unix_ms(spelling).unwrap(), 123);
        }
        for invalid in [
            "19700101T000000Z",
            "1970-01-01T00:00Z",
            "1970-01-01T00:00:00",
            "1970-01-01T00:00:00Z[UTC]",
            "1970-01-01T00:00:00+24:00",
        ] {
            assert!(parse_rfc3339(invalid).is_err(), "{invalid}");
        }
        assert!(rfc3339_to_unix_ms("1969-12-31T23:59:59.9999Z").is_err());
        assert_eq!(
            rfc3339_to_unix_ms("1972-06-30T23:59:60Z").unwrap(),
            rfc3339_to_unix_ms("1972-07-01T00:00:00Z").unwrap()
        );
    }
}
