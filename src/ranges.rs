use crate::types::*;
use crate::{CLOSING_BRACKETS, OPENING_BRACKETS};
use ropey::{Rope, RopeSlice};

pub fn is_end_of_line(idx: usize, rope: &Rope) -> bool {
    if let Some(c) = rope.get_char(idx) {
        return c == '\n';
    }

    true
}

pub fn is_empty_line(rope_line: &RopeSlice) -> bool {
    if rope_line.len_chars() == 1 {
        return true;
    }

    false
}

pub fn cursor_left_idx(cursor_pos: &CursorPos, count: usize, rope: &Rope) -> usize {
    let idx = rope.line_to_char(cursor_pos.y);
    let target_x = cursor_pos.x.saturating_sub(count);
    idx + target_x
}

pub fn cursor_right_idx(cursor_pos: &CursorPos, count: usize, rope: &Rope) -> usize {
    let idx = rope.line_to_char(cursor_pos.y);
    let line = rope.line(cursor_pos.y);
    let target_x = (cursor_pos.x + count).min(line.len_chars().saturating_sub(1));
    idx + target_x
}

pub fn cursor_up_idx(cursor_pos: &CursorPos, count: usize, rope: &Rope) -> usize {
    let target_y = cursor_pos.y.saturating_sub(count);
    let i = rope.line_to_char(target_y);

    // get line length
    let target_line = rope.line(target_y);
    let line_len = target_line.len_chars();

    // prevent x from exceeding the line length
    let target_x = cursor_pos.x.min(line_len.saturating_sub(1));
    return i + target_x;
}

pub fn cursor_down_idx(cursor_pos: &CursorPos, count: usize, rope: &Rope) -> usize {
    // check if cursor is on last line
    let total_lines = rope.len_lines().saturating_sub(2);
    let target_y = (cursor_pos.y + count).min(total_lines);

    if let Ok(i) = rope.try_line_to_char(target_y) {
        // get line length
        let target_line = rope.line(target_y);
        let line_len = target_line.len_chars();

        // prevent x from exceeding the line length
        let target_x = cursor_pos.x.min(line_len.saturating_sub(1));
        return i + target_x;
    } else {
        // go to last line
        let target_y = total_lines;

        // get line length
        let target_line = rope.line(target_y);
        let line_len = target_line.len_chars();

        // prevent x from exceeding the line length
        let target_x = cursor_pos.x.min(line_len.saturating_sub(1));
        rope.line_to_char(target_y) + target_x
    }
}

pub fn first_word_idx(cursor_pos: &CursorPos, rope: &Rope) -> usize {
    let y = cursor_pos.y;
    let line = rope.line(y);
    let mut first_word_idx = rope.line_to_char(y);
    for c in line.chars() {
        if c.is_whitespace() && c != '\n' {
            first_word_idx += 1;
            continue;
        }
        break;
    }

    first_word_idx
}

pub fn next_empty_line_idx(idx: usize, rope: &Rope) -> usize {
    let mut y = rope.char_to_line(idx);
    loop {
        y += 1;
        if let Some(line) = rope.get_line(y) {
            if is_empty_line(&line) {
                break;
            }
        } else {
            return rope.line_to_char(y - 1);
        }
    }
    rope.line_to_char(y)
}

pub fn prev_empty_line_idx(idx: usize, rope: &Rope) -> usize {
    let mut y = rope.char_to_line(idx);
    loop {
        y = y.saturating_sub(1);
        if let Some(line) = rope.get_line(y) {
            if is_empty_line(&line) || y == 0 {
                break;
            }
        } else {
            return 0;
        }
    }
    rope.line_to_char(y)
}

pub fn next_word_idx(mut idx: usize, rope: &Rope, action: bool) -> usize {
    let len = rope.len_chars();
    if idx >= len {
        return idx;
    }

    // 1. Skip current type of characters (word vs non-word)
    let starting_is_alnum = {
        let c = rope.char(idx);
        c.is_alphanumeric() || c == '_'
    };
    while idx < len {
        let c = rope.char(idx);
        if c.is_whitespace() {
            break;
        }
        let current_is_alnum = c.is_alphanumeric() || c == '_';
        if current_is_alnum != starting_is_alnum {
            break;
        }
        idx += 1;
    }

    if action && rope.char(idx) == '\n' {
        return idx;
    }

    // 2. Skip whitespace but stop on empty lines
    while idx < len && rope.char(idx).is_whitespace() {
        if rope.char(idx) == '\n' && idx + 1 < len && rope.char(idx + 1) == '\n' {
            return idx + 1;
        }
        idx += 1;
    }

    idx
}

pub fn upper_word_idx(mut idx: usize, rope: &Rope, action: bool) -> usize {
    let len = rope.len_chars();
    if idx >= len {
        return idx;
    }

    // 1. Skip non-whitespace
    while idx < len && !rope.char(idx).is_whitespace() {
        idx += 1;
    }

    if action && rope.char(idx) == '\n' {
        return idx;
    }

    // 2. Skip whitespace but stop on empty lines
    while idx < len && rope.char(idx).is_whitespace() {
        if rope.char(idx) == '\n' && idx + 1 < len && rope.char(idx + 1) == '\n' {
            return idx + 1;
        }
        idx += 1;
    }

    idx
}

pub fn inside_word(char_idx: usize, rope: &Rope) -> (usize, usize) {
    let mut start_idx = char_idx;
    let mut end_idx = char_idx;
    if rope.char(char_idx).is_whitespace() {
        end_idx += 1;
        return (start_idx, end_idx);
    }
    let starting_is_alnum = {
        let c = rope.char(char_idx);
        c.is_alphanumeric() || c == '_'
    };

    // get start idx
    while start_idx > 0 {
        let prev = rope.char(start_idx - 1);
        if prev.is_whitespace() || prev == '\n' {
            break;
        }
        let prev_is_alnum = prev.is_alphanumeric() || prev == '_';
        if prev_is_alnum != starting_is_alnum {
            break;
        }
        start_idx -= 1;
    }

    // get end idx
    while end_idx + 1 < rope.len_chars() - 1 {
        end_idx += 1;
        let c = rope.char(end_idx);
        if c.is_whitespace() || c == '\n' {
            break;
        }
        let is_alnum = c.is_alphanumeric() || c == '_';
        if is_alnum != starting_is_alnum {
            break;
        }
    }

    (start_idx, end_idx)
}

pub fn inside_upper_word(char_idx: usize, rope: &Rope) -> (usize, usize) {
    let mut start_idx = char_idx;
    let mut end_idx = char_idx;
    if rope.char(char_idx).is_whitespace() {
        end_idx += 1;
        return (start_idx, end_idx);
    }

    // get start idx
    while start_idx > 0 {
        let prev = rope.char(start_idx - 1);
        if prev.is_whitespace() || prev == '\n' {
            break;
        }
        start_idx -= 1;
    }

    // get end idx
    while end_idx + 1 < rope.len_chars() - 1 {
        end_idx += 1;
        let c = rope.char(end_idx);
        if c.is_whitespace() || c == '\n' {
            break;
        }
    }

    (start_idx, end_idx)
}

pub fn word_end_idx(mut idx: usize, rope: &Rope) -> usize {
    let len = rope.len_chars();
    if idx + 1 >= len {
        return idx;
    }

    // 1. Move off current position
    idx += 1;

    // 2. Skip whitespace
    while idx < len && rope.char(idx).is_whitespace() {
        idx += 1;
    }

    if idx >= len {
        return idx;
    }

    // 3. Consume the word — stop when the type changes
    let starting_is_alnum = {
        let c = rope.char(idx);
        c.is_alphanumeric() || c == '_'
    };
    while idx + 1 < len {
        let next = rope.char(idx + 1);
        let next_is_alnum = next.is_alphanumeric() || next == '_';
        if next.is_whitespace() || next_is_alnum != starting_is_alnum {
            break;
        }
        idx += 1;
    }

    idx
}

pub fn upper_word_end_idx(mut idx: usize, rope: &Rope) -> usize {
    let len = rope.len_chars();
    if idx + 1 >= len {
        return idx;
    }

    // 1. Move off current position
    idx += 1;

    // 2. Skip whitespace
    while idx < len && rope.char(idx).is_whitespace() {
        idx += 1;
    }

    if idx >= len {
        return idx;
    }

    // 3. Consume non-whitespace until it changes — stop on last non-whitespace char
    while idx + 1 < len {
        let next = rope.char(idx + 1);
        if next.is_whitespace() {
            break;
        }
        idx += 1;
    }

    idx
}

pub fn prev_word_idx(mut idx: usize, rope: &Rope) -> usize {
    if idx == 0 {
        return 0;
    }

    // 1. Move off current position
    idx -= 1;

    // 2. Skip spaces/tabs but stop at newlines
    while idx > 0 && matches!(rope.char(idx), ' ' | '\t') {
        idx -= 1;
    }

    if idx == 0 {
        return 0;
    }

    // 3. If we're on a newline, check if the previous line is empty (stop) or skip it
    while idx > 0 && rope.char(idx) == '\n' {
        let prev = rope.char(idx - 1);
        if prev == '\n' {
            return idx;
        }
        idx -= 1;
    }

    // 4. Consume characters of the same type going backwards
    let starting_is_alnum = {
        let c = rope.char(idx);
        c.is_alphanumeric() || c == '_'
    };
    while idx > 0 {
        let prev = rope.char(idx - 1);
        if prev.is_whitespace() {
            break;
        }
        let prev_is_alnum = prev.is_alphanumeric() || prev == '_';
        if prev_is_alnum != starting_is_alnum {
            break;
        }
        idx -= 1;
    }

    idx
}

pub fn upper_back_word_idx(mut idx: usize, rope: &Rope) -> usize {
    if idx == 0 {
        return 0;
    }

    // 1. Move off current position
    idx -= 1;

    // 2. Skip spaces/tabs but stop at newlines
    while idx > 0 && matches!(rope.char(idx), ' ' | '\t') {
        idx -= 1;
    }

    // 3. If we're on a newline, check if the previous line is empty (stop) or skip it
    while idx > 0 && rope.char(idx) == '\n' {
        // peek at the char before this newline
        let prev = rope.char(idx - 1);
        if prev == '\n' {
            // empty line — stop here
            return idx;
        }
        idx -= 1;
    }

    // 4. Consume non-whitespace going backwards until we hit whitespace
    while idx > 0 {
        let prev = rope.char(idx - 1);
        if prev.is_whitespace() {
            break;
        }
        idx -= 1;
    }

    idx
}

pub fn line_start_idx(current_line: usize, rope: &Rope) -> usize {
    rope.line_to_char(current_line)
}

pub fn line_end_idx(mut idx: usize, rope: &Rope) -> usize {
    while !is_end_of_line(idx, rope) {
        idx += 1;
    }

    idx
}

pub fn file_end_idx(rope: &Rope) -> usize {
    rope.len_chars().saturating_sub(2)
}

pub fn new_line_below_idx(cursor_pos: &CursorPos, rope: &Rope) -> (usize, String) {
    let y = cursor_pos.y;
    let current_line = rope.line(y);

    // 1. Get leading whitespace
    let whitespace: String = current_line
        .chars()
        .take_while(|c| c.is_whitespace() && *c != '\n' && *c != '\r')
        .collect();

    // 2. Find the end of the current line TEXT (before the \n)
    let line_start_char = rope.line_to_char(y);
    let line_len = current_line.len_chars();

    // We want to skip the \n at the end of the current line if it exists
    let has_newline = current_line
        .chars()
        .last()
        .map_or(false, |c| c == '\n' || c == '\r');
    let insert_pos = if has_newline {
        line_start_char + line_len.saturating_sub(1)
    } else {
        line_start_char + line_len
    };

    (insert_pos, whitespace)
}

pub fn new_line_above_idx(cursor_pos: &CursorPos, rope: &Rope) -> (usize, String) {
    let y = cursor_pos.y;

    // 1. Get leading whitespace from the current line
    let current_line = rope.line(y);
    let whitespace: String = current_line
        .chars()
        .take_while(|c| c.is_whitespace() && *c != '\n' && *c != '\r')
        .collect();

    // 2. Find the start of the current line
    let line_start_char = rope.line_to_char(y);

    (line_start_char, whitespace)
}

pub fn next_search_result_idx(
    char_idx: usize,
    query: &str,
    rope: &Rope,
    line_rng: Option<[usize; 2]>,
) -> Option<usize> {
    // search to the end of the line
    let start_line_idx = rope.char_to_line(char_idx);
    let mut line_idx = start_line_idx;
    if let Some(current_line) = rope.get_line(line_idx) {
        let mut start = char_idx + 1;
        let mut end = char_idx + current_line.len_chars();
        if let Some(slice) = rope.get_slice(start..end) {
            if let Some(idx) = slice.to_string().find(query) {
                return Some(start + idx);
            }
        }

        if let Some(rng) = line_rng {
            start = rng[0];
            end = rng[1];
        } else {
            start = 0;
            end = rope.len_lines().saturating_sub(1);
        }

        // search line by line from cursor
        line_idx += 1;
        while line_idx < end {
            if let Some(text) = rope.get_line(line_idx) {
                if let Some(idx) = text.to_string().find(query) {
                    return Some(rope.line_to_char(line_idx) + idx);
                }
                line_idx += 1;
            } else {
                break;
            }
        }

        // search from the top to the cursor
        line_idx = start;
        while line_idx <= start_line_idx {
            if let Some(text) = rope.get_line(line_idx) {
                if let Some(idx) = text.to_string().find(query) {
                    return Some(rope.line_to_char(line_idx) + idx);
                }
                line_idx += 1;
            } else {
                break;
            }
        }
    }

    None
}

pub fn prev_search_result_idx(char_idx: usize, query: &str, rope: &Rope) -> Option<usize> {
    // search from the start of the line
    let start_line_idx = rope.char_to_line(char_idx);
    let mut line_idx = start_line_idx;
    let start = rope.line_to_char(line_idx);
    let end = char_idx;
    if let Some(idx) = rope.slice(start..end).to_string().rfind(query) {
        return Some(start + idx);
    }

    // search line by line
    while line_idx > 0 {
        line_idx = line_idx.saturating_sub(1);
        let text = rope.line(line_idx).to_string();
        if let Some(idx) = text.rfind(query) {
            return Some(rope.line_to_char(line_idx) + idx);
        }
    }

    // search from the bottom to the cursor
    line_idx = rope.len_lines();
    while line_idx >= start_line_idx {
        line_idx = line_idx.saturating_sub(1);
        let text = rope.line(line_idx).to_string();
        if let Some(idx) = text.rfind(query) {
            return Some(rope.line_to_char(line_idx) + idx);
        }
    }

    None
}

pub fn inside_brackets(
    char_idx: usize,
    rope: &Rope,
    opening: char,
    closing: char,
) -> Option<(usize, usize)> {
    let mut line_limit = 100;
    match opening {
        '{' => {
            line_limit = 1000;
        }
        _ => {}
    }
    let mut start = char_idx;
    let mut end = char_idx;
    let mut idx = char_idx;

    if char_idx >= rope.len_chars() {
        return None;
    }

    // cursor is on '('
    if rope.char(char_idx) == opening {
        let mut count = 0;
        let mut line_count = 0;
        start = char_idx + 1;
        // search forward for end
        idx += 1;
        while idx < rope.len_chars() && line_count < line_limit {
            let c = rope.char(idx);
            if c == closing {
                if count == 0 {
                    end = idx;
                    return Some((start, end));
                } else {
                    count -= 1;
                }
            }

            if c == opening {
                count += 1;
            }

            if c == '\n' {
                line_count += 1;
            }

            idx += 1;
        }

        return None;
    }

    // cursor is on ')'
    if rope.char(char_idx) == closing {
        let mut count = 0;
        let mut line_count = 0;
        end = char_idx;
        // search backwards for '('
        while idx > 0 && line_count < line_limit {
            idx -= 1;
            let c = rope.char(idx);
            if c == opening {
                if count == 0 {
                    start = idx + 1;
                    return Some((start, end));
                } else {
                    count -= 1;
                }
            }

            if c == closing {
                count += 1;
            }

            if c == '\n' {
                line_count += 1;
            }
        }

        return None;
    }

    // check if cursor is inside parens
    let mut found_start = false;
    let mut found_end = false;

    // search backwards for '('
    let mut count = 0;
    let mut line_count = 0;
    while idx > 0 && line_count < line_limit {
        idx -= 1;
        let c = rope.char(idx);
        if c == opening {
            if count == 0 {
                start = idx + 1;
                found_start = true;
                break;
            } else {
                count -= 1;
            }
        }

        if c == closing {
            count += 1;
        }

        if c == '\n' {
            line_count += 1;
        }
    }

    // search for ')'
    if found_start {
        // search forwards for ')' from cursor
        let mut count = 0;
        let mut line_count = 0;
        idx += 1;
        while idx < rope.len_chars() && line_count < line_limit {
            let c = rope.char(idx);
            if c == closing {
                if count == 0 {
                    end = idx;
                    found_end = true;
                    break;
                } else {
                    count -= 1;
                }
            }
            if c == opening {
                count += 1;
            }

            if c == '\n' {
                line_count += 1;
            }

            idx += 1;
        }

        if found_end {
            return Some((start, end));
        }

        return None;
    }

    // find the next parens

    // search forwards for matching '(' and ')'
    idx = char_idx;
    idx += 1;
    let mut count = 0;
    let mut line_count = 0;
    while idx < rope.len_chars() && line_count < line_limit {
        let c = rope.char(idx);
        if found_start {
            if c == opening {
                count += 1;
            } else if c == closing {
                if count == 0 {
                    end = idx;
                    return Some((start, end));
                } else {
                    count -= 1;
                }
            }
        } else if !found_start && c == opening {
            start = idx + 1;
            found_start = true;
        }

        if c == '\n' {
            line_count += 1;
        }

        idx += 1;
    }

    None
}

pub fn inside_quotes(x: usize, y: usize, rope: &Rope, quote: char) -> Option<(usize, usize)> {
    let line_char_idx = rope.line_to_char(y);
    let line = rope.line(y);

    // get ranges in line
    let mut ranges = vec![];
    let mut start = None;
    let mut end = None;
    for (i, c) in line.chars().enumerate() {
        if c == quote {
            if start.is_none() {
                start = Some(i);
            } else if end.is_none() {
                end = Some(i);
                ranges.push((start.unwrap(), end.unwrap()));
                start = None;
                end = None;
            }
        }
    }

    // check if cursor is in a range
    for (start, end) in &ranges {
        if x >= *start && x <= *end {
            return Some((line_char_idx + start + 1, line_char_idx + end));
        }
    }

    // return the next range from the cursor
    for (start, end) in ranges {
        if x <= start {
            return Some((line_char_idx + start + 1, line_char_idx + end));
        }
    }

    None
}

pub fn matching_bracket_idx(cursor_pos: &CursorPos, char_idx: usize, rope: &Rope) -> Option<usize> {
    // if cursor is on bracket
    if let Some(i) = find_matching_bracket(char_idx, rope) {
        return Some(i);
    }

    // if cursor is inside brackets inline
    let x = cursor_pos.x;
    let y = cursor_pos.y;
    let line = rope.line(y);
    let line_char_idx = rope.line_to_char(y);
    let mut idx = x;

    // search backwards for opening bracket
    let mut count = 0;
    while idx > 0 {
        idx -= 1;
        let c = line.char(idx);
        if OPENING_BRACKETS.contains(&c) {
            if count == 0 {
                return Some(line_char_idx + idx);
            }
            count -= 1;
        } else if CLOSING_BRACKETS.contains(&c) {
            count += 1;
        }
    }

    // if outside brackets, find next bracket pair
    let mut found_opening = false;
    idx = x + 1;
    while idx < line.len_chars() - 1 {
        let c = line.char(idx);
        if OPENING_BRACKETS.contains(&c) {
            found_opening = true;
        } else if CLOSING_BRACKETS.contains(&c) && found_opening {
            return Some(line_char_idx + idx);
        }
        idx += 1;
    }

    None
}

/// Takes a char index and a rope and checks the token of the char at the current index.
/// If the token is a bracket, it returns the matching bracket index, otherwise it returns None.
pub fn find_matching_bracket(char_idx: usize, rope: &Rope) -> Option<usize> {
    let token = rope.char(char_idx);
    let mut idx = char_idx;
    let opening;
    let closing;

    if let Some(i) = OPENING_BRACKETS.into_iter().position(|el| el == token) {
        opening = OPENING_BRACKETS[i];
        closing = CLOSING_BRACKETS[i];
    } else if let Some(i) = CLOSING_BRACKETS.into_iter().position(|el| el == token) {
        opening = OPENING_BRACKETS[i];
        closing = CLOSING_BRACKETS[i];
    } else {
        return None;
    }

    if token == opening {
        // search forwards
        let mut count = 0;
        idx += 1;
        while idx < rope.len_chars() - 1 {
            let c = rope.char(idx);
            if c == opening {
                count += 1;
            } else if c == closing {
                if count == 0 {
                    return Some(idx);
                } else {
                    count -= 1;
                }
            }
            idx += 1;
        }
    } else if token == closing {
        // search backwards
        let mut count = 0;
        while idx > 0 {
            idx -= 1;
            let c = rope.char(idx);
            if c == closing {
                count += 1;
            } else if c == opening {
                if count == 0 {
                    return Some(idx);
                } else {
                    count -= 1;
                }
            }
        }
    }

    None
}

pub fn find_char_inline(
    cursor_pos: &CursorPos,
    rope: &Rope,
    query: char,
    forwards: bool,
    inclusive: bool,
) -> Option<usize> {
    let line_char_idx = rope.line_to_char(cursor_pos.y);
    let line = rope.line(cursor_pos.y);
    let mut idx = cursor_pos.x;

    if forwards {
        idx += 1;
        while idx < line.len_chars() {
            let c = line.char(idx);
            if c == query {
                if inclusive {
                    return Some(line_char_idx + idx);
                } else {
                    return Some(line_char_idx + idx - 1);
                }
            }
            idx += 1;
        }
    } else {
        while idx > 0 {
            idx -= 1;
            let c = line.char(idx);
            if c == query {
                if inclusive {
                    return Some(line_char_idx + idx);
                } else {
                    return Some(line_char_idx + idx + 1);
                }
            }
        }
    }

    None
}
