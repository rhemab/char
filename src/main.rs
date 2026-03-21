use std::{env, fs, io};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout},
    prelude::*,
    style::{Color, Style},
};

use ropey::Rope;

use crate::commands::*;

mod commands;
mod trie;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ratatui::run(|terminal| App::default().run(terminal))?;
    Ok(())
}

#[derive(Default)]
pub struct App {
    mode: Mode,
    parser: Parser,
    exit: bool,
    cursor_pos: CursorPos,
    top_line: usize,
    main_height: usize,
    rope: Rope,
    command_bar: String,
    path: String,
}

#[derive(Default, Debug)]
struct CursorPos {
    x: usize,
    y: usize,
    preferred_x: usize,
    preferred_y: usize,
}

#[derive(Debug, PartialEq)]
enum Mode {
    Normal,
    Insert,
    // Visual,
    // VisualLine,
    // VisualBlock,
    Command,
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Normal
    }
}

fn file_position(y: usize, rope: &Rope) -> String {
    let lines = rope.len_lines().saturating_sub(2);

    if y == 0 {
        return "Top".to_string();
    }

    if y == lines {
        return "Bot".to_string();
    }

    let file_percent = (y * 100) / lines;
    format!("{}%", file_percent)
}

fn format_file_size(bytes: usize) -> String {
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

impl App {
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let mut args = env::args();
        args.next();
        if let Some(path) = args.next() {
            // load file
            let rope = Rope::from_reader(io::BufReader::new(fs::File::open(&path)?))?;
            self.rope = rope;
            self.path = path;
        }
        self.command_bar.push_str(&format!(
            "\"{}\" {}L, {}",
            &self.path,
            self.rope.len_lines() - 1,
            format_file_size(self.rope.len_bytes()),
        ));

        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        // update command_bar line based on mode
        match self.mode {
            Mode::Command => {
                self.cursor_pos.y = self.main_height + 2;
                self.cursor_pos.x = self.command_bar.len();
                eprintln!("cursor pos: {:?}", self.cursor_pos);
            }
            Mode::Insert => {
                self.parser.reset();
                self.command_bar.clear();
                self.command_bar.push_str("-- INSERT --");
            }
            // Mode::Visual => {
            //     self.parser.cmd_buffer.clear();
            //     self.command_bar.clear();
            //     self.command_bar.push_str("-- VISUAL --");
            // }
            _ => {}
        }
        use Constraint::{Length, Min};
        let vertical = Layout::vertical([Min(1), Length(1), Length(1)]);
        let [main_area, status_bar, command_bar_area] = vertical.areas(frame.area());
        let status_style = Style::new().bg(Color::DarkGray);

        let height = main_area.height as usize;
        self.main_height = height;

        let cursor_x = self.cursor_pos.x;
        let cursor_y = if self.mode != Mode::Command {
            self.cursor_pos.y.saturating_sub(self.top_line)
        } else {
            self.cursor_pos.y
        };

        let start_idx = self.top_line;
        let end_idx = (start_idx + height).min(self.rope.len_lines());

        // convert rope slice to ratatui line
        let mut lines = Vec::new();
        for i in start_idx..end_idx {
            if let Some(rope_line) = self.rope.get_line(i as usize) {
                lines.push(Line::from(rope_line.to_string()));
            }
        }

        // content of status bar
        let text_content = Text::from(lines);
        let file_path_content = Line::from(self.path.clone()).left_aligned();
        let cursor_location_content = Line::from(format!(
            "{},{}    {}",
            self.cursor_pos.y + 1,
            self.cursor_pos.x + 1,
            file_position(self.cursor_pos.y, &self.rope),
        ))
        .right_aligned();

        // content of command bar
        let command_bar_content = Line::from(self.command_bar.clone());
        let command_buffer_content =
            Line::from(format!("{}    ", self.parser.input_buffer.clone())).right_aligned();

        // render main content
        frame.render_widget(text_content, main_area);

        // render status bar
        frame.render_widget(file_path_content.style(status_style), status_bar);
        frame.render_widget(cursor_location_content.style(status_style), status_bar);

        // render command bar
        frame.render_widget(command_bar_content, command_bar_area);
        frame.render_widget(command_buffer_content, command_bar_area);

        // render cursor
        frame.set_cursor_position((cursor_x as u16, cursor_y as u16));
    }

    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            // it's important to check that the event is a key press event as
            // crossterm also emits key release and repeat events on Windows.
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event)
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        // escape always return to normal from anywhere
        match (key_event.code, key_event.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('['), KeyModifiers::CONTROL) => {
                match self.mode {
                    Mode::Insert => {
                        // if exiting insert mode, move cursor left 1
                        self.cursor_pos.x = self.cursor_pos.x.saturating_sub(1);
                    }
                    Mode::Command => {
                        // if exiting command mode put cursor back
                        self.cursor_pos.y = self.cursor_pos.preferred_y;
                        self.cursor_pos.x = self.cursor_pos.preferred_x;
                    }
                    _ => {}
                }
                self.return_to_normal_mode();
                return;
            }
            _ => {}
        }
        match &mut self.mode {
            Mode::Command => match key_event.code {
                KeyCode::Enter => match self.command_bar.as_str() {
                    ":q" => {
                        self.exit();
                    }
                    _ => {
                        self.cursor_pos.y = self.cursor_pos.preferred_y;
                        self.cursor_pos.x = self.cursor_pos.preferred_x;
                        self.return_to_normal_mode();
                    }
                },
                KeyCode::Char(c) => self.command_bar.push(c),
                KeyCode::Backspace => {
                    self.command_bar.pop();
                    if self.command_bar.is_empty() {
                        self.cursor_pos.y = self.cursor_pos.preferred_y;
                        self.cursor_pos.x = self.cursor_pos.preferred_x;
                        self.return_to_normal_mode();
                    }
                }
                _ => {}
            },
            Mode::Normal => {
                if let Some(command) = self.parser.generate_command(key_event) {
                    self.parser.command = None;
                    self.parser.input_buffer.clear();
                    self.parser.motion_buffer.clear();

                    eprintln!("Command: {:?}", command);

                    let mut should_update_preferred_x = false;
                    let char_idx = self.rope.line_to_char(self.cursor_pos.y) + self.cursor_pos.x;
                    let mut range = (char_idx, char_idx);
                    let mut cursor_target_idx = char_idx;
                    let mut count = 1;
                    if let Ok(n) = command.count.parse::<usize>() {
                        count = n;
                    }

                    // check for motion
                    match command.motion {
                        Some(Motion::EnterCommandMode) => {
                            self.command_bar.clear();
                            self.command_bar.push(':');
                            self.cursor_pos.preferred_y = self.cursor_pos.y;
                            self.cursor_pos.preferred_x = self.cursor_pos.x;
                            self.mode = Mode::Command;
                            return;
                        }
                        Some(Motion::FileStart) => {
                            range = (0, char_idx);
                            cursor_target_idx = 0;
                            should_update_preferred_x = true;
                        }
                        Some(Motion::Insert) => {
                            self.mode = Mode::Insert;
                            return;
                        }
                        Some(Motion::UpperInsert) => {
                            cursor_target_idx = first_word_idx(&self.cursor_pos, &self.rope);
                            self.update_cursor_from_char_idx(cursor_target_idx);
                            self.mode = Mode::Insert;
                            return;
                        }
                        Some(Motion::Append) => {
                            self.cursor_pos.x += 1;
                            self.mode = Mode::Insert;
                            return;
                        }
                        Some(Motion::UpperAppend) => {
                            if is_empty_line(char_idx, &self.rope) {
                                self.mode = Mode::Insert;
                                return;
                            }
                            cursor_target_idx = line_end_idx(char_idx, &self.rope);
                            self.update_cursor_from_char_idx(cursor_target_idx);
                            self.cursor_pos.x += 1;
                            self.mode = Mode::Insert;
                            return;
                        }
                        Some(Motion::Left) => {
                            if is_empty_line(char_idx, &self.rope) {
                                return;
                            }
                            let mut cursor_adjust = 0;
                            match command.action {
                                Some(Action::Delete) => {
                                    cursor_adjust = count;
                                }
                                _ => {}
                            }
                            range = (
                                cursor_left_idx(&self.cursor_pos, count, &self.rope),
                                char_idx,
                            );
                            cursor_target_idx = range.0.saturating_sub(cursor_adjust);
                            eprintln!("cursor_target: {}", cursor_target_idx);
                            should_update_preferred_x = true;
                            self.cursor_pos.x = self.cursor_pos.x.saturating_sub(count);
                        }
                        Some(Motion::Right) => {
                            if is_empty_line(char_idx, &self.rope) {
                                return;
                            }
                            range = (
                                char_idx,
                                cursor_right_idx(&self.cursor_pos, count, &self.rope),
                            );
                            cursor_target_idx = range.1;
                            should_update_preferred_x = true;
                        }
                        Some(Motion::Up) => {
                            // dk should delete two whole lines
                            self.cursor_pos.x = self.cursor_pos.preferred_x;
                            range = (char_idx, cursor_up_idx(&self.cursor_pos, count, &self.rope));
                            cursor_target_idx = range.1;
                        }
                        Some(Motion::Down) => {
                            // dj should delete two whole lines
                            self.cursor_pos.x = self.cursor_pos.preferred_x;
                            range = (
                                char_idx,
                                cursor_down_idx(&self.cursor_pos, count, &self.rope),
                            );
                            cursor_target_idx = range.1;
                        }
                        Some(Motion::HalfScreenUp) => {
                            self.cursor_pos.x = self.cursor_pos.preferred_x;
                            range = (
                                char_idx,
                                cursor_up_idx(&self.cursor_pos, self.main_height / 2, &self.rope),
                            );
                            cursor_target_idx = range.1;
                        }
                        Some(Motion::HalfScreenDown) => {
                            self.cursor_pos.x = self.cursor_pos.preferred_x;
                            range = (
                                char_idx,
                                cursor_down_idx(&self.cursor_pos, self.main_height / 2, &self.rope),
                            );
                            cursor_target_idx = range.1;
                        }
                        Some(Motion::Word) => {
                            for _ in 0..count {
                                range = (char_idx, next_word_idx(range.1, &self.rope));
                            }
                            cursor_target_idx = range.1;
                            should_update_preferred_x = true;
                        }
                        Some(Motion::UpperWord) => {
                            for _ in 0..count {
                                range = (char_idx, upper_word_idx(range.1, &self.rope));
                            }
                            cursor_target_idx = range.1;
                            should_update_preferred_x = true;
                        }
                        Some(Motion::End) => {
                            let mut range_end = char_idx;
                            for _ in 0..count {
                                range_end = word_end_idx(range_end, &self.rope);
                            }
                            range = (char_idx, range_end + 1);
                            cursor_target_idx = range_end;
                            should_update_preferred_x = true;
                        }
                        Some(Motion::UpperEnd) => {
                            for _ in 0..count {
                                range = (char_idx, upper_word_end_idx(range.1, &self.rope));
                            }
                            cursor_target_idx = range.1;
                            should_update_preferred_x = true;
                        }
                        Some(Motion::Back) => {
                            for _ in 0..count {
                                range = (prev_word_idx(range.0, &self.rope), char_idx);
                            }
                            cursor_target_idx = range.0;
                            should_update_preferred_x = true;
                        }
                        Some(Motion::UpperBack) => {
                            for _ in 0..count {
                                range = (upper_back_word_idx(range.0, &self.rope), char_idx);
                            }
                            cursor_target_idx = range.0;
                            should_update_preferred_x = true;
                        }
                        Some(Motion::FirstWord) => {
                            cursor_target_idx = first_word_idx(&self.cursor_pos, &self.rope);
                            range = (
                                char_idx.min(cursor_target_idx),
                                char_idx.max(cursor_target_idx),
                            );
                            should_update_preferred_x = true;
                        }
                        Some(Motion::LineStart) => {
                            range = (line_start_idx(self.cursor_pos.y, &self.rope), char_idx);
                            cursor_target_idx = range.0;
                            should_update_preferred_x = true;
                        }
                        Some(Motion::LineEnd) => {
                            if is_empty_line(char_idx, &self.rope) {
                                return;
                            }
                            cursor_target_idx = line_end_idx(char_idx, &self.rope);
                            range = (char_idx, cursor_target_idx + 1);
                            self.cursor_pos.preferred_x = usize::MAX;
                        }
                        Some(Motion::FileEnd) => {
                            range = (char_idx, file_end_idx(&self.rope));
                            cursor_target_idx = range.1;
                            should_update_preferred_x = true;
                        }
                        Some(Motion::NewLineBelow) => {
                            let (insert_pos, whitespace) =
                                new_line_below_idx(&self.cursor_pos, &self.rope);
                            self.rope.insert(insert_pos, &format!("\n{}", whitespace));
                            self.cursor_pos.y += 1;
                            self.cursor_pos.x = whitespace.chars().count();
                            self.mode = Mode::Insert;
                            return;
                        }
                        Some(Motion::NewLineAbove) => {
                            let (insert_pos, whitespace) =
                                new_line_above_idx(&self.cursor_pos, &self.rope);
                            let insert_str = format!("{}\n", whitespace);
                            self.rope.insert(insert_pos, &insert_str);
                            self.cursor_pos.x = whitespace.chars().count();
                            self.mode = Mode::Insert;
                            return;
                        }
                        _ => {}
                    }

                    eprintln!("range: {:?}", range);
                    eprintln!("preferred x: {:?}", self.cursor_pos.preferred_x);

                    // check for action
                    match command.action {
                        Some(Action::Delete) => {
                            // delete range
                            self.rope.remove(range.0..range.1);
                            cursor_target_idx = range.0;
                            self.cursor_pos.preferred_x = self.cursor_pos.x;
                        }
                        _ => {}
                    }

                    self.update_cursor_from_char_idx(cursor_target_idx);
                    self.ensure_valid_normal_pos();
                    self.scroll();

                    if should_update_preferred_x {
                        self.cursor_pos.preferred_x = self.cursor_pos.x;
                    }
                }
            }
            Mode::Insert => self.insert_text(key_event),
        }
    }

    fn insert_text(&mut self, e: KeyEvent) {
        match e.code {
            KeyCode::Char(c) => {
                let i = self.rope.line_to_char(self.cursor_pos.y);
                let x = self.cursor_pos.x;
                self.rope.insert_char(i + x, c);
                self.cursor_pos.x += 1;
            }
            KeyCode::Tab => {
                let i = self.rope.line_to_char(self.cursor_pos.y);
                let x = self.cursor_pos.x;
                self.rope.insert(i + x, "    ");
                self.cursor_pos.x += 4;
            }
            KeyCode::Backspace => {
                let x = self.cursor_pos.x;
                let y = self.cursor_pos.y;

                if x > 0 {
                    // NORMAL BACKSPACE: Just delete the char to the left
                    let char_idx = self.rope.line_to_char(y) + x;
                    self.rope.remove(char_idx - 1..char_idx);
                    self.cursor_pos.x -= 1;
                } else if y > 0 {
                    // LINE MERGE: Backspacing at the start of a line

                    // 1. Get the length of the previous line before we merge
                    // We subtract 1 from y to look at the line above
                    let prev_line_len = self.rope.line(y - 1).len_chars();

                    // 2. Find the index of the newline character
                    // In Ropey, the newline is usually the last char of the line
                    let char_idx = self.rope.line_to_char(y);

                    // 3. Remove the newline character
                    self.rope.remove(char_idx - 1..char_idx);

                    // 4. Move cursor up to the end of the previous line
                    self.cursor_pos.y -= 1;

                    // If the previous line had a \n, the cursor should be
                    // just before it. Ropey's line length includes the \n.
                    self.cursor_pos.x = prev_line_len - 1;
                }
            }
            KeyCode::Enter => {
                let i = self.rope.line_to_char(self.cursor_pos.y);
                let x = self.cursor_pos.x;
                self.rope.insert_char(i + x, '\n');
                self.cursor_pos.y += 1;
                self.cursor_pos.x = 0;
            }
            _ => {}
        }
    }

    fn scroll(&mut self) {
        let offset = 8;
        let height = self.main_height - 1 - offset;
        // don't let cursor go beyond file length
        self.cursor_pos.y = self
            .cursor_pos
            .y
            .min(self.rope.len_lines().saturating_sub(2));

        let y = self.cursor_pos.y;

        if y.saturating_sub(self.top_line) >= height {
            // scroll down
            self.top_line = y.saturating_sub(height);
        } else if y <= self.top_line + offset {
            // scroll up
            self.top_line = y.saturating_sub(offset);
        }
    }

    fn return_to_normal_mode(&mut self) {
        self.mode = Mode::Normal;
        self.command_bar.clear();
        self.parser.input_buffer.clear();
        self.parser.motion_buffer.clear();
        self.ensure_valid_normal_pos();
        self.cursor_pos.preferred_x = self.cursor_pos.x;
    }

    fn exit(&mut self) {
        self.exit = true;
    }

    fn ensure_valid_normal_pos(&mut self) {
        let line = self.rope.line(self.cursor_pos.y);
        let line_len = line.len_chars();

        // If the line is "Hello\n", len is 6.
        // In Insert mode, x can be 5 (after 'o').
        // In Normal mode, x must be at most 4 ('o').

        let has_newline =
            line_len > 0 && (line.char(line_len - 1) == '\n' || line.char(line_len - 1) == '\r');

        let max_x = if has_newline {
            // -1 to get index, -1 to stay off the \n
            line_len.saturating_sub(2)
        } else {
            // If no newline (EOF), just -1 for index
            line_len.saturating_sub(1)
        };

        if self.cursor_pos.x > max_x {
            self.cursor_pos.x = max_x;
        }
    }

    fn update_cursor_from_char_idx(&mut self, char_idx: usize) {
        let total_chars = self.rope.len_chars();
        let safe_idx = char_idx.min(total_chars.saturating_sub(2));

        self.cursor_pos.y = self.rope.char_to_line(safe_idx);
        self.cursor_pos.x = safe_idx - self.rope.line_to_char(self.cursor_pos.y);
    }
}

// helpers
fn is_end_of_line(idx: usize, rope: &Rope) -> bool {
    let len = rope.len_chars();
    if idx >= len {
        return true;
    }
    rope.char(idx) == '\n' || idx + 1 >= len || rope.char(idx + 1) == '\n'
}

fn is_empty_line(idx: usize, rope: &Rope) -> bool {
    let len = rope.len_chars();
    if rope.char(idx) == '\n' && idx + 1 < len && rope.char(idx + 1) == '\n' {
        return true;
    }
    false
}

// get ranges
fn cursor_left_idx(cursor_pos: &CursorPos, count: usize, rope: &Rope) -> usize {
    let idx = rope.line_to_char(cursor_pos.y);
    let target_x = cursor_pos.x.saturating_sub(count);
    idx + target_x
}

fn cursor_right_idx(cursor_pos: &CursorPos, count: usize, rope: &Rope) -> usize {
    let idx = rope.line_to_char(cursor_pos.y);
    let line = rope.line(cursor_pos.y);
    let target_x = (cursor_pos.x + count).min(line.len_chars().saturating_sub(1));
    idx + target_x
}

fn cursor_up_idx(cursor_pos: &CursorPos, count: usize, rope: &Rope) -> usize {
    // rope.line_to_char(cursor_pos.y.saturating_sub(count))
    let target_y = cursor_pos.y.saturating_sub(count);
    let i = rope.line_to_char(target_y);

    // get line length
    let target_line = rope.line(target_y);
    let line_len = target_line.len_chars();

    // prevent x from exceeding the line length
    let target_x = cursor_pos.x.min(line_len.saturating_sub(2));
    return i + target_x;
}

fn cursor_down_idx(cursor_pos: &CursorPos, count: usize, rope: &Rope) -> usize {
    // check if cursor is on last line
    let total_lines = rope.len_lines().saturating_sub(2);
    let target_y = (cursor_pos.y + count).min(total_lines);

    if let Ok(i) = rope.try_line_to_char(target_y) {
        // get line length
        let target_line = rope.line(target_y);
        let line_len = target_line.len_chars();

        // prevent x from exceeding the line length
        let target_x = cursor_pos.x.min(line_len.saturating_sub(2));
        return i + target_x;
    } else {
        // go to last line
        let target_y = total_lines;

        // get line length
        let target_line = rope.line(target_y);
        let line_len = target_line.len_chars();

        // prevent x from exceeding the line length
        let target_x = cursor_pos.x.min(line_len.saturating_sub(2));
        rope.line_to_char(target_y) + target_x
    }
}

fn first_word_idx(cursor_pos: &CursorPos, rope: &Rope) -> usize {
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

fn next_word_idx(mut idx: usize, rope: &Rope) -> usize {
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

    // 2. Skip whitespace but stop on empty lines
    while idx < len && rope.char(idx).is_whitespace() {
        if rope.char(idx) == '\n' && idx + 1 < len && rope.char(idx + 1) == '\n' {
            return idx + 1;
        }
        idx += 1;
    }

    idx
}

fn upper_word_idx(mut idx: usize, rope: &Rope) -> usize {
    let len = rope.len_chars();
    if idx >= len {
        return idx;
    }

    // 1. Skip non-whitespace
    while idx < len && !rope.char(idx).is_whitespace() {
        idx += 1;
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

fn word_end_idx(mut idx: usize, rope: &Rope) -> usize {
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

fn upper_word_end_idx(mut idx: usize, rope: &Rope) -> usize {
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

fn prev_word_idx(mut idx: usize, rope: &Rope) -> usize {
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

fn upper_back_word_idx(mut idx: usize, rope: &Rope) -> usize {
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

fn line_start_idx(current_line: usize, rope: &Rope) -> usize {
    // get char idx of cursor.y
    rope.line_to_char(current_line)
}

fn line_end_idx(current_idx: usize, rope: &Rope) -> usize {
    let len = rope.len_chars();
    let mut idx = current_idx;
    while idx < len {
        if is_end_of_line(idx, rope) {
            break;
        }
        idx += 1;
    }

    idx
}

fn file_end_idx(rope: &Rope) -> usize {
    rope.line_to_char(rope.len_lines().saturating_sub(2))
}

fn new_line_below_idx(cursor_pos: &CursorPos, rope: &Rope) -> (usize, String) {
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

fn new_line_above_idx(cursor_pos: &CursorPos, rope: &Rope) -> (usize, String) {
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
