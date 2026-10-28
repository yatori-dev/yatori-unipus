pub(super) fn to_text(s: &str) -> String {
    let mut out = Vec::new();
    for line in s.lines() {
        let t = line.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("WEBVTT") {
            continue;
        }
        if t.contains("-->") {
            continue;
        }
        if t.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        out.push(t);
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtt_parse_strips_timestamps() {
        let vtt = "WEBVTT\n\n1\n00:00:07.299 --> 00:00:08.564\nHi, everyone!\n\n2\n00:00:08.914 --> 00:00:11.891\nWelcome!\n";
        let text = to_text(vtt);
        assert_eq!(text, "Hi, everyone!\nWelcome!");
        assert!(!text.contains("-->"));
    }
}
