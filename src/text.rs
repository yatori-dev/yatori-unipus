pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut result: String = s.chars().take(n).collect();
        result.push('…');
        result
    }
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncates_by_characters() {
        assert_eq!(truncate("中a文", 2), "中a…");
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("abc", 0), "…");
    }
}
