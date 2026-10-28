pub fn format_unix_minute(secs: i64) -> String {
    if secs <= 0 {
        return "未知".into();
    }
    time::OffsetDateTime::from_unix_timestamp(secs)
        .ok()
        .and_then(|t| {
            t.format(time::macros::format_description!(
                "[year]-[month]-[day] [hour]:[minute]"
            ))
            .ok()
        })
        .unwrap_or_else(|| secs.to_string())
}

#[cfg(test)]
mod tests {
    use super::format_unix_minute;

    #[test]
    fn formats_known_and_missing_timestamps() {
        assert_eq!(format_unix_minute(0), "未知");
        assert_eq!(format_unix_minute(1_790_000_000), "2026-09-21 14:13");
    }
}
