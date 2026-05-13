pub fn format_file_size(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = 1024 * KB;
    const GB: usize = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// extract search query from command string
pub fn extract_query(s: &str) -> String {
    let mut query = String::new();
    let mut start = false;
    for c in s.chars() {
        if c == '/' {
            if !start {
                start = true;
            } else {
                break;
            }
        } else if start {
            query.push(c);
        }
    }

    query
}

/// extract replacment text from command string
pub fn extract_replacment(s: &str) -> String {
    let mut replacment = String::new();
    let mut count = 0;
    for c in s.chars() {
        if c == '/' {
            count += 1;
            if count > 2 {
                break;
            }
        } else if count > 1 {
            replacment.push(c);
        }
    }

    replacment
}
