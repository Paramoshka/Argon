pub struct Location {
    pub proxy_pass: String,
}

impl Location {
    pub fn extract_location_blocks(server_block: &str) -> Vec<String> {
        let mut blocks = Vec::new();
        let mut inside = false;
        let mut brace_count = 0;
        let mut current_block = String::new();

        for line in server_block.lines() {
            let trimmed = line.trim();

            if !inside && trimmed.starts_with("location ") && trimmed.contains('{') {
                inside = true;
                brace_count += trimmed.matches('{').count();
                current_block.push_str(line);
                current_block.push('\n');
                continue;
            }

            if inside {
                brace_count += trimmed.matches('{').count();
                brace_count -= trimmed.matches('}').count();

                current_block.push_str(line);
                current_block.push('\n');

                if brace_count == 0 {
                    blocks.push(current_block.trim().to_string());
                    current_block.clear();
                    inside = false;
                }
            }
        }

        blocks
    }

    pub fn extract_location_path(location_block: &str) -> Option<String> {
        for line in location_block.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("location ") {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 2 {
                    return Some(parts[1].to_string());
                }
            }
        }
        None
    }


}