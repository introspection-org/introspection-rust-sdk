/// RFC 3339 UTC, to the second, for `epoch` seconds since the Unix epoch.
pub(crate) fn rfc3339(epoch: i64) -> String {
    let days = epoch.div_euclid(86_400);
    let secs = epoch.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.000Z",
        secs / 3_600,
        secs % 3_600 / 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn epochs_format_as_javascripts_to_iso_string() {
        assert_eq!(super::rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(super::rfc3339(1_791_000_000), "2026-10-03T04:00:00.000Z");
        assert_eq!(super::rfc3339(951_782_400), "2000-02-29T00:00:00.000Z");
    }
}
