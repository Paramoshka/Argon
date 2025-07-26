
pub struct ServerBlock {
    pub block: Vec<String>,
}

impl ServerBlock {
    pub fn extract_server_blocks(input: &str) -> Vec<String> {
        let mut blocks = Vec::new();
        let mut inside = false;
        let mut brace_count = 0;
        let mut current_block = String::new();

        for line in input.lines() {
            let trimmed = line.trim();

            // Начало блока
            if !inside && trimmed.starts_with("server") && trimmed.contains('{') {
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
                    current_block = String::new();
                    inside = false;
                }
            }
        }

        blocks
    }
}


