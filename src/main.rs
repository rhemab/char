use std::{collections::HashMap, env, fs, io};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout},
    prelude::*,
    style::{Color, Style},
    widgets::Block,
};

use ropey::{Rope, RopeSlice};

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
    last_command: commands::Command,
    last_insertion: String,
    redraw: bool,
    dirty: bool,
    mode: Mode,
    parser: Parser,
    cursor_pos: CursorPos,
    top_line: usize,
    main_height: usize,
    rope: Rope,
    command_bar: String,
    path: String,
    selection: VisualSelection,
    yank_buffer: HashMap<char, YankBuffer>,
    highlight_yank: bool,
    query: String,
}

#[derive(Clone)]
enum YankBuffer {
    Chars(String),
    Lines(String),
}

#[derive(Default, Debug)]
struct VisualSelection {
    ancor: usize,
    cursor: usize,
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
    Visual,
    // VisualLine,
    // VisualBlock,
    Command,
    Search,
    Exit,
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Normal
    }
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
    fn file_position(&self) -> String {
        let y = if self.mode == Mode::Command {
            self.cursor_pos.preferred_y
        } else {
            self.cursor_pos.y
        };
        let lines = self.rope.len_lines().saturating_sub(2);

        if lines <= self.main_height {
            return "Top".to_string();
        }

        if y == 0 {
            return "Top".to_string();
        }

        if y == lines {
            return "Bot".to_string();
        }

        let file_percent = (y * 100) / lines;
        format!("{}%", file_percent)
    }

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

        self.yank_buffer
            .insert('"', YankBuffer::Chars(String::new()));

        while self.mode != Mode::Exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        if self.highlight_yank {
            self.redraw = true;
        }
        match self.mode {
            Mode::Command | Mode::Search => {
                self.cursor_pos.y = self.main_height + 2;
                self.cursor_pos.x = self.command_bar.len();
            }
            _ => {}
        }
        use Constraint::{Length, Min};
        let vertical = Layout::vertical([Min(1), Length(1), Length(1)]);
        let [main_area, status_bar, command_bar_area] = vertical.areas(frame.area());
        let status_style = Style::new().bg(Color::DarkGray);

        let height = main_area.height as usize;
        self.main_height = height;

        let start_line_idx = self.top_line;
        let end_line_idx = (start_line_idx + height).min(self.rope.len_lines());

        let start_select_rng = self.selection.ancor.min(self.selection.cursor);
        let end_select_rng = self.selection.ancor.max(self.selection.cursor);

        // convert rope slice to ratatui line
        let mut lines = Vec::new();
        let mut line_nums = vec![];
        for line_num in start_line_idx..end_line_idx {
            if let Some(rope_line) = self.rope.get_line(line_num as usize) {
                let line_length = rope_line.len_chars();
                let line_start_char = self.rope.line_to_char(line_num);
                let line_end_char = line_start_char + line_length;

                let line_in_selection = (self.mode == Mode::Visual || self.highlight_yank)
                    && line_end_char > start_select_rng
                    && line_start_char <= end_select_rng;

                if line_in_selection {
                    let mut line_of_spans = vec![];
                    let mut char_buffer = String::new();
                    let mut highlighting = false;
                    for (char_idx, c) in rope_line.chars().enumerate() {
                        if line_length == 1 && c == '\n' {
                            line_of_spans.push(Span::raw(" ").fg(Color::White).bg(Color::DarkGray));
                            continue;
                        }
                        let abs_idx = line_start_char + char_idx;
                        let in_select_rng =
                            abs_idx >= start_select_rng && abs_idx <= end_select_rng;
                        if in_select_rng {
                            if !highlighting && !char_buffer.is_empty() {
                                line_of_spans.push(Span::raw(char_buffer.clone()));
                                char_buffer.clear();
                            }
                            highlighting = true;
                            char_buffer.push(c);
                        } else {
                            if highlighting && !char_buffer.is_empty() {
                                line_of_spans.push(
                                    Span::raw(char_buffer.clone())
                                        .fg(Color::White)
                                        .bg(Color::DarkGray),
                                );
                                char_buffer.clear();
                            }
                            highlighting = false;
                            char_buffer.push(c);
                        }
                    }
                    if !char_buffer.is_empty() {
                        if highlighting {
                            line_of_spans.push(
                                Span::raw(char_buffer.clone())
                                    .fg(Color::White)
                                    .bg(Color::DarkGray),
                            );
                        } else {
                            line_of_spans.push(Span::raw(char_buffer.clone()));
                        }
                    }

                    lines.push(Line::from(line_of_spans));
                } else {
                    lines.push(Line::from(rope_line.to_string()));
                }

                // generate line numbers
                // don't show last ropey line
                if line_num >= self.rope.len_lines() - 1 {
                    continue;
                }
                let line_number = if line_num == self.cursor_pos.y
                    || self.mode == Mode::Command
                    || self.mode == Mode::Search
                {
                    format!("{} ", line_num + 1) // absolute, 1-indexed
                } else {
                    format!(
                        "{}",
                        (line_num as isize - self.cursor_pos.y as isize).unsigned_abs()
                    )
                };
                line_nums.push(Line::from(line_number));
            }
        }

        self.highlight_yank = false;

        let n = self.rope.len_lines();
        let digits = if n == 0 { 1 } else { n.ilog10() + 2 };
        let gap = 1;
        let horizontal = Layout::horizontal([Length((digits) as u16), Length(gap), Min(1)]);
        let [num_col, gap_col, text_area] = horizontal.areas(main_area);

        let x_offset = digits + gap as u32;
        let cursor_x = if self.mode == Mode::Command || self.mode == Mode::Search {
            self.cursor_pos.x
        } else {
            self.cursor_pos.x + x_offset as usize
        };
        let cursor_y = if self.mode == Mode::Command || self.mode == Mode::Search {
            self.cursor_pos.y
        } else {
            self.cursor_pos.y.saturating_sub(self.top_line)
        };

        // content of status bar
        let text_content = Text::from(lines);
        let line_nums = Text::from(line_nums).alignment(Alignment::Right);
        let file_path_content = if self.dirty {
            Line::from(format!("{} [+]", self.path.clone())).left_aligned()
        } else {
            Line::from(self.path.clone()).left_aligned()
        };
        let cursor_location_content = if self.mode != Mode::Command && self.mode != Mode::Search {
            Line::from(format!(
                "{},{}    {}",
                self.cursor_pos.y + 1,
                self.cursor_pos.x + 1,
                self.file_position(),
            ))
            .right_aligned()
        } else {
            Line::from(format!(
                "{},{}    {}",
                self.cursor_pos.preferred_y + 1,
                self.cursor_pos.preferred_x + 1,
                self.file_position(),
            ))
            .right_aligned()
        };

        // content of command bar
        let command_bar_content = Line::from(self.command_bar.clone());
        let command_buffer_content =
            Line::from(format!("{}    ", self.parser.input_buffer.clone())).right_aligned();

        // render main content
        frame.render_widget(line_nums.style(Style::new().fg(Color::DarkGray)), num_col);
        frame.render_widget(Block::new(), gap_col);
        frame.render_widget(text_content.style(Style::new().fg(Color::Gray)), text_area);

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
        if self.redraw {
            match event::poll(std::time::Duration::from_millis(150)) {
                Ok(false) => {
                    self.redraw = false;
                    return Ok(());
                }
                _ => {}
            }
        }
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
                    Mode::Command | Mode::Search => {
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
            Mode::Command | Mode::Search => match key_event.code {
                KeyCode::Enter => {
                    match self.command_bar.as_str() {
                        ":q" => {
                            self.exit();
                            return;
                        }
                        _ => {
                            if self.mode == Mode::Search {
                                let query = &self.command_bar.as_str()[1..];
                                self.query = query.to_string();
                                let char_idx = self.rope.line_to_char(self.cursor_pos.preferred_y)
                                    + self.cursor_pos.preferred_x;
                                let cursor_target_idx =
                                    next_search_result_idx(char_idx, query, &self.rope);
                                self.update_cursor_from_char_idx(cursor_target_idx);
                                self.cursor_pos.preferred_y = self.cursor_pos.y;
                                self.cursor_pos.preferred_x = self.cursor_pos.x;
                            }
                        }
                    }
                    self.cursor_pos.y = self.cursor_pos.preferred_y;
                    self.cursor_pos.x = self.cursor_pos.preferred_x;
                    self.return_to_normal_mode();
                    self.scroll();
                }
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
            Mode::Insert => self.insert_text(key_event),
            _ => {
                let visual_mode = self.mode == Mode::Visual;
                if let Some(command) = self.parser.generate_command(key_event, visual_mode) {
                    self.execute_command(command, visual_mode, false);
                }
            }
        }
    }

    fn execute_command(&mut self, command: commands::Command, visual_mode: bool, repeat: bool) {
        self.parser.reset();

        eprintln!("Command: {:?}", command);

        let action = command.action.is_some();
        let mut yank_lines = false;
        let mut should_update_preferred_x = false;
        let mut should_move_cursor = true;
        let mut should_save_command = false;
        let char_idx = self.rope.line_to_char(self.cursor_pos.y) + self.cursor_pos.x;
        let mut range = (char_idx, char_idx);
        let mut cursor_target_idx = char_idx;
        let mut count = 1;
        if let Ok(n) = command.count.parse::<usize>() {
            count = n;
        }

        // check for motion
        match (command.motion, command.modifier) {
            (Some(Motion::EnterSearchMode), _) => {
                self.cursor_pos.preferred_y = self.cursor_pos.y;
                self.cursor_pos.preferred_x = self.cursor_pos.x;
                self.change_mode(Mode::Search);
                return;
            }
            (Some(Motion::EnterCommandMode), _) => {
                self.cursor_pos.preferred_y = self.cursor_pos.y;
                self.cursor_pos.preferred_x = self.cursor_pos.x;
                self.change_mode(Mode::Command);
                return;
            }
            (Some(Motion::FileStart), _) => {
                range = (0, char_idx);
                cursor_target_idx = 0;
                should_update_preferred_x = true;
            }
            (Some(Motion::VisualMode), _) => {
                self.selection.ancor = char_idx;
                self.selection.cursor = char_idx;
                self.change_mode(Mode::Visual);
                return;
            }
            (Some(Motion::InsertMode), _) => {
                should_save_command = true;
                self.change_mode(Mode::Insert);
                should_move_cursor = false;
            }
            (Some(Motion::UpperInsert), _) => {
                should_save_command = true;
                cursor_target_idx = first_word_idx(&self.cursor_pos, &self.rope);
                self.update_cursor_from_char_idx(cursor_target_idx);
                self.change_mode(Mode::Insert);
                should_move_cursor = false;
            }
            (Some(Motion::Append), _) => {
                should_save_command = true;
                self.cursor_pos.x += 1;
                self.change_mode(Mode::Insert);
                should_move_cursor = false;
            }
            (Some(Motion::UpperAppend), _) => {
                should_save_command = true;
                let rope_line = self.rope.line(self.cursor_pos.y);
                if is_empty_line(&rope_line) {
                    self.change_mode(Mode::Insert);
                }
                cursor_target_idx = line_end_idx(char_idx, &self.rope);
                self.update_cursor_from_char_idx(cursor_target_idx);
                self.cursor_pos.x += 1;
                self.change_mode(Mode::Insert);
                should_move_cursor = false;
            }
            (Some(Motion::Left), _) => {
                if self.cursor_pos.x == 0 {
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
                should_update_preferred_x = true;
                self.cursor_pos.x = self.cursor_pos.x.saturating_sub(count);
            }
            (Some(Motion::Right), _) => {
                let rope_line = self.rope.line(self.cursor_pos.y);
                if is_empty_line(&rope_line) {
                    return;
                }
                range = (
                    char_idx,
                    cursor_right_idx(&self.cursor_pos, count, &self.rope),
                );
                cursor_target_idx = range.1;
                should_update_preferred_x = true;
            }
            (Some(Motion::Up), _) => {
                // dk should delete two whole lines
                self.cursor_pos.x = self.cursor_pos.preferred_x;
                range = (char_idx, cursor_up_idx(&self.cursor_pos, count, &self.rope));
                cursor_target_idx = range.1;
            }
            (Some(Motion::Down), _) => {
                // dj should delete two whole lines
                self.cursor_pos.x = self.cursor_pos.preferred_x;
                range = (
                    char_idx,
                    cursor_down_idx(&self.cursor_pos, count, &self.rope),
                );
                cursor_target_idx = range.1;
            }
            (Some(Motion::HalfScreenUp), _) => {
                self.cursor_pos.x = self.cursor_pos.preferred_x;
                range = (
                    char_idx,
                    cursor_up_idx(&self.cursor_pos, self.main_height / 2, &self.rope),
                );
                cursor_target_idx = range.1;
            }
            (Some(Motion::HalfScreenDown), _) => {
                self.cursor_pos.x = self.cursor_pos.preferred_x;
                range = (
                    char_idx,
                    cursor_down_idx(&self.cursor_pos, self.main_height / 2, &self.rope),
                );
                cursor_target_idx = range.1;
            }
            (Some(Motion::NextEmptyLine), _) => {
                for _ in 0..count {
                    range = (char_idx, next_empty_line_idx(range.1, &self.rope));
                }
                cursor_target_idx = range.1;
            }
            (Some(Motion::PrevEmptyLine), _) => {
                for _ in 0..count {
                    range = (prev_empty_line_idx(range.0, &self.rope), char_idx);
                }
                cursor_target_idx = range.0;
            }
            (Some(Motion::Backtick), Some(modifier)) => {
                if let Some(r) =
                    inside_quotes(self.cursor_pos.x, self.cursor_pos.y, &self.rope, '`')
                {
                    match modifier {
                        commands::Modifier::Around => {
                            range = (r.0 - 1, r.1 + 1);
                        }
                        _ => {
                            range = r;
                        }
                    }
                    cursor_target_idx = range.0;
                    should_update_preferred_x = true;
                } else {
                    return;
                }
            }
            (Some(Motion::SingleQuote), Some(modifier)) => {
                if let Some(r) =
                    inside_quotes(self.cursor_pos.x, self.cursor_pos.y, &self.rope, '\'')
                {
                    match modifier {
                        commands::Modifier::Around => {
                            range = (r.0 - 1, r.1 + 1);
                        }
                        _ => {
                            range = r;
                        }
                    }
                    cursor_target_idx = range.0;
                    should_update_preferred_x = true;
                } else {
                    return;
                }
            }
            (Some(Motion::DoubleQuote), Some(modifier)) => {
                if let Some(r) =
                    inside_quotes(self.cursor_pos.x, self.cursor_pos.y, &self.rope, '"')
                {
                    match modifier {
                        commands::Modifier::Around => {
                            range = (r.0 - 1, r.1 + 1);
                        }
                        _ => {
                            range = r;
                        }
                    }
                    cursor_target_idx = range.0;
                    should_update_preferred_x = true;
                } else {
                    return;
                }
            }
            (Some(Motion::OpenAngleBracket), Some(modifier)) => {
                if let Some(r) = inside_delimiter(char_idx, &self.rope, '<', '>') {
                    match modifier {
                        commands::Modifier::Around => {
                            range = (r.0 - 1, r.1 + 1);
                        }
                        _ => {
                            range = r;
                        }
                    }
                    cursor_target_idx = range.0;
                    should_update_preferred_x = true;
                } else {
                    return;
                }
            }
            (Some(Motion::OpenCurlyBrace), Some(modifier)) => {
                if let Some(r) = inside_delimiter(char_idx, &self.rope, '{', '}') {
                    match modifier {
                        commands::Modifier::Around => {
                            range = (r.0 - 1, r.1 + 1);
                        }
                        _ => {
                            range = r;
                        }
                    }
                    cursor_target_idx = range.0;
                    should_update_preferred_x = true;
                } else {
                    return;
                }
            }
            (Some(Motion::OpenBracket), Some(modifier)) => {
                if let Some(r) = inside_delimiter(char_idx, &self.rope, '[', ']') {
                    match modifier {
                        commands::Modifier::Around => {
                            range = (r.0 - 1, r.1 + 1);
                        }
                        _ => {
                            range = r;
                        }
                    }
                    cursor_target_idx = range.0;
                    should_update_preferred_x = true;
                } else {
                    return;
                }
            }
            (Some(Motion::OpenParen), Some(modifier)) => {
                if let Some(r) = inside_delimiter(char_idx, &self.rope, '(', ')') {
                    match modifier {
                        commands::Modifier::Around => {
                            range = (r.0 - 1, r.1 + 1);
                        }
                        _ => {
                            range = r;
                        }
                    }
                    cursor_target_idx = range.0;
                    should_update_preferred_x = true;
                } else {
                    return;
                }
            }
            (Some(Motion::Word), Some(commands::Modifier::Inside)) => {
                let rope_line = self.rope.line(self.cursor_pos.y);
                if is_empty_line(&rope_line) {
                    self.last_command = command.clone();
                    return;
                }
                range = inside_word(char_idx, &self.rope);
                cursor_target_idx = range.0;
                should_update_preferred_x = true;
            }
            (Some(Motion::Word), None) => {
                // delete, change, and yank should stop at \n
                for _ in 0..count {
                    range = (char_idx, next_word_idx(range.1, &self.rope, action));
                }
                cursor_target_idx = range.1;
                should_update_preferred_x = true;
            }
            (Some(Motion::UpperWord), Some(commands::Modifier::Inside)) => {
                let rope_line = self.rope.line(self.cursor_pos.y);
                if is_empty_line(&rope_line) {
                    self.last_command = command.clone();
                    return;
                }
                range = inside_upper_word(char_idx, &self.rope);
                cursor_target_idx = range.0;
                should_update_preferred_x = true;
            }
            (Some(Motion::UpperWord), _) => {
                for _ in 0..count {
                    range = (char_idx, upper_word_idx(range.1, &self.rope, action));
                }
                cursor_target_idx = range.1;
                should_update_preferred_x = true;
            }
            (Some(Motion::End), _) => {
                let mut range_end = char_idx;
                for _ in 0..count {
                    range_end = word_end_idx(range_end, &self.rope);
                }
                range = (char_idx, range_end + 1);
                cursor_target_idx = range_end;
                should_update_preferred_x = true;
            }
            (Some(Motion::UpperEnd), _) => {
                for _ in 0..count {
                    range = (char_idx, upper_word_end_idx(range.1, &self.rope));
                }
                cursor_target_idx = range.1;
                should_update_preferred_x = true;
            }
            (Some(Motion::Back), _) => {
                for _ in 0..count {
                    range = (prev_word_idx(range.0, &self.rope), char_idx);
                }
                cursor_target_idx = range.0;
                should_update_preferred_x = true;
            }
            (Some(Motion::UpperBack), _) => {
                for _ in 0..count {
                    range = (upper_back_word_idx(range.0, &self.rope), char_idx);
                }
                cursor_target_idx = range.0;
                should_update_preferred_x = true;
            }
            (Some(Motion::FirstWord), _) => {
                cursor_target_idx = first_word_idx(&self.cursor_pos, &self.rope);
                range = (
                    char_idx.min(cursor_target_idx),
                    char_idx.max(cursor_target_idx),
                );
                should_update_preferred_x = true;
            }
            (Some(Motion::LineStart), _) => {
                range = (line_start_idx(self.cursor_pos.y, &self.rope), char_idx);
                cursor_target_idx = range.0;
                should_update_preferred_x = true;
            }
            (Some(Motion::LineEnd), _) => {
                let rope_line = self.rope.line(self.cursor_pos.y);
                if is_empty_line(&rope_line) {
                    return;
                }
                cursor_target_idx = line_end_idx(char_idx, &self.rope);
                range = (char_idx, cursor_target_idx);
                self.cursor_pos.preferred_x = usize::MAX;
            }
            (Some(Motion::FileEnd), _) => {
                range = (char_idx, file_end_idx(&self.rope));
                cursor_target_idx = range.1;
                should_update_preferred_x = true;
            }
            (Some(Motion::NewLineBelow), _) => {
                should_save_command = true;
                let (insert_pos, whitespace) = new_line_below_idx(&self.cursor_pos, &self.rope);
                self.rope.insert(insert_pos, &format!("\n{}", whitespace));
                self.cursor_pos.y += 1;
                self.cursor_pos.x = whitespace.chars().count();
                self.change_mode(Mode::Insert);
                should_move_cursor = false;
            }
            (Some(Motion::NewLineAbove), _) => {
                should_save_command = true;
                let (insert_pos, whitespace) = new_line_above_idx(&self.cursor_pos, &self.rope);
                let insert_str = format!("{}\n", whitespace);
                self.rope.insert(insert_pos, &insert_str);
                self.cursor_pos.x = whitespace.chars().count();
                self.change_mode(Mode::Insert);
                should_move_cursor = false;
            }
            (Some(Motion::DeleteLine), _) | (Some(Motion::YankLine), _) => {
                should_save_command = true;
                range = (
                    self.rope.line_to_char(self.cursor_pos.y),
                    self.rope.line_to_char(self.cursor_pos.y + 1),
                );
                yank_lines = true;
            }
            (Some(Motion::ChangeLine), _) => {
                should_save_command = true;
                range = (
                    self.rope.line_to_char(self.cursor_pos.y),
                    self.rope.line_to_char(self.cursor_pos.y + 1) - 1,
                );
                yank_lines = true;
            }
            (Some(Motion::UpperChange), _) => {
                let rope_line = self.rope.line(self.cursor_pos.y);
                if is_empty_line(&rope_line) {
                    self.change_mode(Mode::Insert);
                    return;
                }
                cursor_target_idx = line_end_idx(char_idx, &self.rope);
                range = (char_idx, cursor_target_idx);
                should_move_cursor = false;
            }
            (Some(Motion::Paste), _) => {
                should_save_command = true;
                if let Some(buf) = self.yank_buffer.get(&'"') {
                    match buf {
                        YankBuffer::Chars(content) => {
                            let mut insert_idx = char_idx;
                            // if on empty line, insert before cursor
                            if self.rope.char(char_idx) != '\n' {
                                insert_idx += 1;
                            }
                            self.rope.insert(insert_idx, &content);
                            cursor_target_idx = char_idx + content.len();
                        }
                        YankBuffer::Lines(content) => {
                            // insert line below
                            let idx = self.rope.line_to_char(self.cursor_pos.y + 1);
                            self.rope.insert(idx, &content);
                            cursor_target_idx = idx;
                        }
                    }
                }
            }
            (Some(Motion::UpperPaste), _) => {
                should_save_command = true;
                if let Some(buf) = self.yank_buffer.get(&'"') {
                    match buf {
                        YankBuffer::Chars(content) => {
                            // insert before cursor
                            self.rope.insert(char_idx, &content);
                            cursor_target_idx = char_idx + content.len() - 1;
                        }
                        YankBuffer::Lines(content) => {
                            // insert line above
                            let idx = self.rope.line_to_char(self.cursor_pos.y.saturating_sub(1));
                            self.rope.insert(idx, &content);
                            cursor_target_idx = idx;
                        }
                    }
                }
            }
            (Some(Motion::NextSearchResult), _) => {
                cursor_target_idx = next_search_result_idx(char_idx, &self.query, &self.rope);
                should_update_preferred_x = true;
            }
            (Some(Motion::PrevSearchResult), _) => {
                cursor_target_idx = prev_search_result_idx(char_idx, &self.query, &self.rope);
                should_update_preferred_x = true;
            }
            (Some(Motion::Repeat), _) => {
                self.execute_command(self.last_command.clone(), visual_mode, true);
                if self.mode == Mode::Insert {
                    let idx = self.rope.line_to_char(self.cursor_pos.y) + self.cursor_pos.x;
                    self.rope.insert(idx, &self.last_insertion);
                    self.update_cursor_from_char_idx(idx + self.last_insertion.len() - 1);
                    self.ensure_valid_normal_pos();
                }
                self.change_mode(Mode::Normal);
                return;
            }
            _ => {}
        }

        // check for yank
        match command.action {
            Some(Action::Yank) | Some(Action::Delete) | Some(Action::Change) => {
                // yank slice to buffer
                if let Some(slice) = self.rope.get_slice(range.0..range.1) {
                    let new_content = if yank_lines {
                        YankBuffer::Lines(String::from(slice))
                    } else {
                        YankBuffer::Chars(String::from(slice))
                    };
                    self.yank_buffer
                        .entry('"')
                        .and_modify(|content| *content = new_content.clone())
                        .or_insert(new_content);
                    self.selection.cursor = range.0;
                    self.selection.ancor = range.1.saturating_sub(1);
                }
            }
            _ => {}
        }

        // check for action
        match command.action {
            Some(Action::Yank) => {
                self.highlight_yank = true;
                self.cursor_pos.preferred_x = self.cursor_pos.x;
                self.cursor_pos.preferred_y = self.cursor_pos.y;
                return;
            }
            Some(Action::Delete) | Some(Action::Change) => {
                should_save_command = true;
                if visual_mode {
                    let start_select_rng = self.selection.ancor.min(self.selection.cursor);
                    let end_select_rng = self.selection.ancor.max(self.selection.cursor);
                    range = (start_select_rng, end_select_rng + 1);
                }
                // delete range
                self.rope.remove(range.0..range.1);
                self.cursor_pos.preferred_x = self.cursor_pos.x;
                cursor_target_idx = range.0;

                match command.action {
                    Some(Action::Change) => {
                        self.change_mode(Mode::Insert);
                    }
                    _ => {
                        self.change_mode(Mode::Normal);
                    }
                }
            }
            _ => {}
        }

        if self.mode == Mode::Insert && !repeat {
            self.last_insertion.clear();
        }

        if should_save_command {
            self.last_command = command.clone();
        }

        if should_move_cursor {
            self.update_cursor_from_char_idx(cursor_target_idx);
            self.ensure_valid_normal_pos();
            self.selection.cursor = self.rope.line_to_char(self.cursor_pos.y) + self.cursor_pos.x;
        }

        if should_update_preferred_x {
            self.cursor_pos.preferred_x = self.cursor_pos.x;
        }

        self.scroll();
    }

    fn insert_text(&mut self, e: KeyEvent) {
        let mut text_to_insert = None;
        let mut idx = 0;
        match e.code {
            KeyCode::Char(c) => {
                let i = self.rope.line_to_char(self.cursor_pos.y);
                let x = self.cursor_pos.x;
                idx = i + x;
                text_to_insert = Some(String::from(c));
                self.cursor_pos.x += 1;
            }
            KeyCode::Tab => {
                let i = self.rope.line_to_char(self.cursor_pos.y);
                let x = self.cursor_pos.x;
                idx = i + x;
                text_to_insert = Some(String::from("    "));
                self.cursor_pos.x += 4;
            }
            KeyCode::Backspace => {
                let x = self.cursor_pos.x;
                let y = self.cursor_pos.y;
                self.last_insertion.pop();

                if x > 0 {
                    // NORMAL BACKSPACE: Just delete the char to the left
                    idx = self.rope.line_to_char(y) + x;
                    self.rope.remove(idx - 1..idx);
                    self.cursor_pos.x -= 1;
                } else if y > 0 {
                    // LINE MERGE: Backspacing at the start of a line

                    // 1. Get the length of the previous line before we merge
                    // We subtract 1 from y to look at the line above
                    let prev_line_len = self.rope.line(y - 1).len_chars();

                    // 2. Find the index of the newline character
                    // In Ropey, the newline is usually the last char of the line
                    let idx = self.rope.line_to_char(y);

                    // 3. Remove the newline character
                    self.rope.remove(idx - 1..idx);

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
                idx = i + x;
                text_to_insert = Some(String::from('\n'));
                self.cursor_pos.y += 1;
                self.cursor_pos.x = 0;
            }
            _ => {}
        }
        if let Some(text) = text_to_insert {
            self.rope.insert(idx, &text);
            self.last_insertion += &text;
        }
        self.scroll();
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
        self.change_mode(Mode::Normal);
        self.parser.reset();
        self.ensure_valid_normal_pos();
        self.cursor_pos.preferred_x = self.cursor_pos.x;
    }

    fn exit(&mut self) {
        self.mode = Mode::Exit;
    }

    fn change_mode(&mut self, target_mode: Mode) {
        match target_mode {
            Mode::Normal => {
                if self.mode != Mode::Search {
                    self.command_bar.clear();
                }
            }
            Mode::Search => {
                self.command_bar.clear();
                self.command_bar.push_str("/");
            }
            Mode::Command => {
                self.command_bar.clear();
                self.command_bar.push_str(":");
            }
            Mode::Insert => {
                self.command_bar.clear();
                self.command_bar.push_str("-- INSERT --");
            }
            Mode::Visual => {
                self.command_bar.clear();
                self.command_bar.push_str("-- VISUAL --");
            }
            _ => {}
        }

        self.mode = target_mode;
    }

    fn ensure_valid_normal_pos(&mut self) {
        if self.mode == Mode::Visual {
            return;
        }
        if let Some(line) = self.rope.get_line(self.cursor_pos.y) {
            let line_len = line.len_chars();

            // If the line is "Hello\n", len is 6.
            // In Insert mode, x can be 5 (after 'o').
            // In Normal mode, x must be at most 4 ('o').

            let has_newline = line_len > 0
                && (line.char(line_len - 1) == '\n' || line.char(line_len - 1) == '\r');

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
    }

    fn update_cursor_from_char_idx(&mut self, char_idx: usize) {
        let total_chars = self.rope.len_chars();
        let safe_idx = char_idx.min(total_chars.saturating_sub(1));

        self.cursor_pos.y = self.rope.char_to_line(safe_idx);
        self.cursor_pos.x = safe_idx - self.rope.line_to_char(self.cursor_pos.y);
    }
}

// helpers
fn is_end_of_line(idx: usize, rope: &Rope) -> bool {
    rope.char(idx) == '\n'
}

fn is_empty_line(rope_line: &RopeSlice) -> bool {
    if rope_line.len_chars() == 1 {
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
    let target_y = cursor_pos.y.saturating_sub(count);
    let i = rope.line_to_char(target_y);

    // get line length
    let target_line = rope.line(target_y);
    let line_len = target_line.len_chars();

    // prevent x from exceeding the line length
    let target_x = cursor_pos.x.min(line_len.saturating_sub(1));
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

fn next_empty_line_idx(idx: usize, rope: &Rope) -> usize {
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

fn prev_empty_line_idx(idx: usize, rope: &Rope) -> usize {
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

fn next_word_idx(mut idx: usize, rope: &Rope, action: bool) -> usize {
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

fn upper_word_idx(mut idx: usize, rope: &Rope, action: bool) -> usize {
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

fn inside_word(char_idx: usize, rope: &Rope) -> (usize, usize) {
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

fn inside_upper_word(char_idx: usize, rope: &Rope) -> (usize, usize) {
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
    rope.line_to_char(current_line)
}

fn line_end_idx(mut idx: usize, rope: &Rope) -> usize {
    while !is_end_of_line(idx, rope) {
        idx += 1;
    }

    idx
}

fn file_end_idx(rope: &Rope) -> usize {
    rope.len_chars().saturating_sub(2)
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

fn next_search_result_idx(char_idx: usize, query: &str, rope: &Rope) -> usize {
    // search to the end of the line
    let start_line_idx = rope.char_to_line(char_idx);
    let mut line_idx = start_line_idx;
    let current_line = rope.line(line_idx);
    let start = char_idx + 1;
    let end = char_idx + current_line.len_chars();
    if let Some(idx) = rope.slice(start..end).to_string().find(query) {
        return start + idx;
    }

    // search line by line
    let total_lines = rope.len_lines();
    line_idx += 1;
    while line_idx < total_lines {
        let text = rope.line(line_idx).to_string();
        if let Some(idx) = text.find(query) {
            return rope.line_to_char(line_idx) + idx;
        }
        line_idx += 1;
    }

    // search from the top to the cursor
    line_idx = 0;
    while line_idx <= start_line_idx {
        let text = rope.line(line_idx).to_string();
        if let Some(idx) = text.find(query) {
            return rope.line_to_char(line_idx) + idx;
        }
        line_idx += 1;
    }

    char_idx
}

fn prev_search_result_idx(char_idx: usize, query: &str, rope: &Rope) -> usize {
    // search from the start of the line
    let start_line_idx = rope.char_to_line(char_idx);
    let mut line_idx = start_line_idx;
    let start = rope.line_to_char(line_idx);
    let end = char_idx;
    if let Some(idx) = rope.slice(start..end).to_string().rfind(query) {
        return start + idx;
    }

    // search line by line
    while line_idx > 0 {
        line_idx = line_idx.saturating_sub(1);
        let text = rope.line(line_idx).to_string();
        if let Some(idx) = text.rfind(query) {
            return rope.line_to_char(line_idx) + idx;
        }
    }

    // search from the bottom to the cursor
    line_idx = rope.len_lines();
    while line_idx >= start_line_idx {
        line_idx = line_idx.saturating_sub(1);
        let text = rope.line(line_idx).to_string();
        if let Some(idx) = text.rfind(query) {
            return rope.line_to_char(line_idx) + idx;
        }
    }

    char_idx
}

fn inside_delimiter(
    char_idx: usize,
    rope: &Rope,
    opening: char,
    closing: char,
) -> Option<(usize, usize)> {
    let mut line_limit = 100;
    match opening {
        '{' => {
            line_limit = 500;
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

fn inside_quotes(x: usize, y: usize, rope: &Rope, quote: char) -> Option<(usize, usize)> {
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
