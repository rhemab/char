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

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ratatui::run(|terminal| App::default().run(terminal))?;
    Ok(())
}

#[derive(Default, Debug)]
pub struct App {
    mode: Mode,
    parser: commands::Parser,
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
}

#[derive(Debug)]
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
        use Constraint::{Length, Min};
        let vertical = Layout::vertical([Min(1), Length(1), Length(1)]);
        let [main_area, status_bar, command_bar_area] = vertical.areas(frame.area());
        let status_style = Style::new().bg(Color::DarkGray);

        let height = main_area.height as usize;
        self.main_height = height;

        let cursor_x = self.cursor_pos.x;
        let cursor_y = self.cursor_pos.y.saturating_sub(self.top_line);

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
            Line::from(format!("{}    ", self.parser.cmd_buffer.clone())).right_aligned();

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

    /// updates the application's state based on user input
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
                        self.return_to_normal_mode();
                    }
                },
                KeyCode::Char(c) => self.command_bar.push(c),
                _ => {}
            },
            Mode::Normal => {
                match key_event.code {
                    KeyCode::Char(':') => {
                        self.command_bar.clear();
                        self.command_bar.push(':');
                        self.mode = Mode::Command;
                        return;
                    }
                    _ => {}
                }
                if let Some(command) = self.parser.generate_command(key_event.code) {
                    self.parser.command = None;
                    self.parser.cmd_buffer.clear();
                    eprintln!("Command: {:?}", command);
                    // get range
                    let char_idx = self.rope.line_to_char(self.cursor_pos.y) + self.cursor_pos.x;
                    let mut range = (char_idx, char_idx);
                    let mut cursor_target_idx;
                    let mut count = 1;
                    if let Ok(n) = command.count.parse::<usize>() {
                        count = n;
                    }

                    // check for global cmd
                    match command.global {
                        Some(Global::FileStart) => {
                            range = (0, char_idx);
                            cursor_target_idx = 0;
                            self.update_cursor_from_char_idx(cursor_target_idx);
                            self.cursor_pos.preferred_x = self.cursor_pos.x;
                        }
                        _ => {}
                    }

                    // check for motion
                    match command.motion {
                        Some(Motion::InsertMode) => {
                            self.mode = Mode::Insert;
                        }
                        Some(Motion::Left) => {
                            range = (
                                cursor_left_idx(&self.cursor_pos, count, &self.rope),
                                char_idx,
                            );
                            cursor_target_idx = range.0;
                            self.update_cursor_from_char_idx(cursor_target_idx);
                            self.cursor_pos.preferred_x = self.cursor_pos.x;
                        }
                        Some(Motion::Right) => {
                            range = (
                                char_idx,
                                cursor_right_idx(&self.cursor_pos, count, &self.rope),
                            );
                            cursor_target_idx = range.1;
                            self.update_cursor_from_char_idx(cursor_target_idx);
                            self.cursor_pos.preferred_x = self.cursor_pos.x;
                        }
                        Some(Motion::Up) => {
                            self.cursor_pos.x = self.cursor_pos.preferred_x;
                            range = (char_idx, cursor_up_idx(&self.cursor_pos, count, &self.rope));
                            cursor_target_idx = range.1;
                            self.update_cursor_from_char_idx(cursor_target_idx);
                        }
                        Some(Motion::Down) => {
                            self.cursor_pos.x = self.cursor_pos.preferred_x;
                            range = (
                                char_idx,
                                cursor_down_idx(&self.cursor_pos, count, &self.rope),
                            );
                            cursor_target_idx = range.1;
                            self.update_cursor_from_char_idx(cursor_target_idx);
                        }
                        Some(Motion::Word) => {
                            for _ in 0..count {
                                range = (char_idx, next_word_idx(range.1, &self.rope));
                            }
                            cursor_target_idx = range.1;
                            self.update_cursor_from_char_idx(cursor_target_idx);
                            self.cursor_pos.preferred_x = self.cursor_pos.x;
                        }
                        Some(Motion::Back) => {
                            for _ in 0..count {
                                range = (back_word_idx(range.0, &self.rope), char_idx);
                            }
                            cursor_target_idx = range.0;
                            self.update_cursor_from_char_idx(cursor_target_idx);
                            self.cursor_pos.preferred_x = self.cursor_pos.x;
                        }
                        Some(Motion::FirstWord) => {
                            eprintln!("cursor_pos: {:?}", self.cursor_pos);
                            cursor_target_idx = first_word_idx(&self.cursor_pos, &self.rope);
                            range = (
                                char_idx.min(cursor_target_idx),
                                char_idx.max(cursor_target_idx),
                            );
                            self.update_cursor_from_char_idx(cursor_target_idx);
                            self.cursor_pos.preferred_x = self.cursor_pos.x;
                        }
                        Some(Motion::LineStart) => {
                            range = (line_start_idx(self.cursor_pos.y, &self.rope), char_idx);
                            cursor_target_idx = range.0;
                            self.update_cursor_from_char_idx(cursor_target_idx);
                            self.cursor_pos.preferred_x = self.cursor_pos.x;
                        }
                        Some(Motion::LineEnd) => {
                            range = (0, line_end_idx(char_idx, &self.rope));
                            cursor_target_idx = range.1;
                            self.update_cursor_from_char_idx(cursor_target_idx);
                            self.cursor_pos.preferred_x = usize::MAX;
                        }
                        Some(Motion::FileEnd) => {
                            range = (char_idx, file_end_idx(&self.rope));
                            cursor_target_idx = range.1;
                            self.update_cursor_from_char_idx(cursor_target_idx);
                            self.cursor_pos.preferred_x = self.cursor_pos.x;
                        }
                        _ => {}
                    }

                    // match action & excecute on range
                    match command.action {
                        Some(Action::Delete) => {}
                        Some(Action::Change) => {}
                        _ => {
                            eprintln!("range: {:?}", range);
                        }
                    }

                    self.scroll();
                }

                // update command_bar line based on mode
                match self.mode {
                    Mode::Insert => {
                        self.parser.cmd_buffer.clear();
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
            }
            Mode::Insert => self.insert_text(key_event),
            // Mode::Visual => match key_event.code {
            //     _ => {
            //         // self.process_motion(key_event.code);
            //     }
            // },
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

    fn open_line_below(&mut self) {
        let y = self.cursor_pos.y;
        let current_line = self.rope.line(y);

        // 1. Get leading whitespace
        let whitespace: String = current_line
            .chars()
            .take_while(|c| c.is_whitespace() && *c != '\n' && *c != '\r')
            .collect();

        // 2. Find the end of the current line TEXT (before the \n)
        let line_start_char = self.rope.line_to_char(y);
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

        // 3. Insert \n and the same whitespace
        self.rope.insert(insert_pos, &format!("\n{}", whitespace));

        // 4. Update cursor
        self.cursor_pos.y += 1;
        self.cursor_pos.x = whitespace.chars().count();

        self.mode = Mode::Insert;
    }

    fn open_line_above(&mut self) {
        let y = self.cursor_pos.y;

        // 1. Get leading whitespace from the current line
        let current_line = self.rope.line(y);
        let whitespace: String = current_line
            .chars()
            .take_while(|c| c.is_whitespace() && *c != '\n' && *c != '\r')
            .collect();

        // 2. Find the start of the current line
        let line_start_char = self.rope.line_to_char(y);

        // 3. Insert indentation THEN the newline
        // This places the new text "above" the current content
        let insert_str = format!("{}\n", whitespace);
        self.rope.insert(line_start_char, &insert_str);

        // 4. Update cursor
        // y stays the same because the "new" line is now at the old y index
        // x moves to the end of the whitespace
        self.cursor_pos.x = whitespace.chars().count();

        self.mode = Mode::Insert;
    }

    fn move_upper_word_forward(&mut self) {
        let mut char_idx = self.rope.line_to_char(self.cursor_pos.y) + self.cursor_pos.x;
        let total_chars = self.rope.len_chars();

        if char_idx >= total_chars.saturating_sub(1) {
            return;
        }

        // 1. Skip all non-whitespace characters (the current WORD)
        while char_idx < total_chars && !self.rope.char(char_idx).is_whitespace() {
            char_idx += 1;
        }

        // 2. Skip all whitespace characters to land at the start of the next WORD
        while char_idx < total_chars && self.rope.char(char_idx).is_whitespace() {
            char_idx += 1;
        }

        self.update_cursor_from_char_idx(char_idx);
        self.cursor_pos.preferred_x = self.cursor_pos.x;
    }

    fn move_upper_word_backward(&mut self) {
        let mut char_idx = self.rope.line_to_char(self.cursor_pos.y) + self.cursor_pos.x;
        if char_idx == 0 {
            return;
        }

        // 1. Skip whitespace to the left to find a WORD
        while char_idx > 0 && self.rope.char(char_idx - 1).is_whitespace() {
            char_idx -= 1;
        }

        // 2. Move left until we hit whitespace or start of file
        while char_idx > 0 && !self.rope.char(char_idx - 1).is_whitespace() {
            char_idx -= 1;
        }

        self.update_cursor_from_char_idx(char_idx);
        self.cursor_pos.preferred_x = self.cursor_pos.x;
    }

    fn move_upper_word_end(&mut self) {
        let mut char_idx = self.rope.line_to_char(self.cursor_pos.y) + self.cursor_pos.x;
        let total_chars = self.rope.len_chars();

        if char_idx >= total_chars.saturating_sub(1) {
            return;
        }
        char_idx += 1;

        // 1. Skip whitespace to find the start of the next WORD
        while char_idx < total_chars && self.rope.char(char_idx).is_whitespace() {
            char_idx += 1;
        }

        // 2. Move forward until the character BEFORE a whitespace
        while char_idx < total_chars.saturating_sub(1)
            && !self.rope.char(char_idx + 1).is_whitespace()
        {
            char_idx += 1;
        }

        self.update_cursor_from_char_idx(char_idx);
        self.cursor_pos.preferred_x = self.cursor_pos.x;
    }

    fn delete_current_line(&mut self) {
        let y = self.cursor_pos.y;
        let total_lines = self.rope.len_lines();

        if total_lines == 0 {
            return;
        }

        let start_idx = self.rope.line_to_char(y);

        // The end index is the start of the NEXT line.
        // If there is no next line, it's the end of the rope.
        let end_idx = if y + 1 < total_lines {
            self.rope.line_to_char(y + 1)
        } else {
            self.rope.len_chars()
        };

        self.rope.remove(start_idx..end_idx);

        // If we deleted the last line, move cursor up.
        // Otherwise, keep y the same (the line below just moved up into this slot).
        if y >= self.rope.len_lines() - 1 && y > 0 {
            self.cursor_pos.y -= 1;
        }
        // self.cursor_first_word();
    }
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
    let target_x = (cursor_pos.x + count).min(line.len_chars().saturating_sub(2));
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
        eprintln!("going to next line");
        // get line length
        let target_line = rope.line(target_y);
        let line_len = target_line.len_chars();

        // prevent x from exceeding the line length
        let target_x = cursor_pos.x.min(line_len.saturating_sub(2));
        return i + target_x;
    } else {
        eprintln!("going to last line");
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
    let mut iter = rope.chars_at(idx);

    // 1. Skip current "type" of characters (word vs non-word)
    if let Some(first_char) = iter.next() {
        idx += 1;
        let starting_is_alnum = first_char.is_alphanumeric() || first_char == '_';

        for c in iter {
            let current_is_alnum = c.is_alphanumeric() || c == '_';
            if current_is_alnum != starting_is_alnum || c.is_whitespace() {
                break;
            }
            idx += 1;
        }
    }

    // 2. Skip any trailing whitespace to land at the start of the next word
    let iter = rope.chars_at(idx);
    for c in iter {
        if !c.is_whitespace() {
            break;
        }
        idx += 1;
    }

    idx
}

fn back_word_idx(mut idx: usize, rope: &Rope) -> usize {
    if idx == 0 {
        return idx;
    }

    // 1. Skip whitespace to the left
    while idx > 0 {
        let c = rope.char(idx - 1);
        if !c.is_whitespace() {
            break;
        }
        idx -= 1;
    }

    if idx == 0 {
        return idx;
    }

    // 2. Determine character type (alphanumeric vs punctuation)
    let first_char = rope.char(idx - 1);
    let target_is_alnum = first_char.is_alphanumeric() || first_char == '_';

    // 3. Move left until the type changes or we hit whitespace
    while idx > 0 {
        let c = rope.char(idx - 1);
        let current_is_alnum = c.is_alphanumeric() || c == '_';

        if c.is_whitespace() || current_is_alnum != target_is_alnum {
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
    let iter = rope.chars_at(current_idx);
    let mut idx = current_idx;

    for c in iter {
        if c == '\n' {
            idx -= 1;
            break;
        }
        idx += 1;
    }

    idx
}

fn file_end_idx(rope: &Rope) -> usize {
    rope.line_to_char(rope.len_lines().saturating_sub(2))
}
// testing
