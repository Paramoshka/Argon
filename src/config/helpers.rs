pub fn parse_directive(block: &str, directive: &str) -> Option<String> {
    block.lines()
        .map(str::trim)
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            match (parts.next(), parts.next()) {
                (Some(key), Some(value)) if key == directive => {
                    Some(value.trim_end_matches(';').to_string())
                }
                _ => None,
            }
        })
}