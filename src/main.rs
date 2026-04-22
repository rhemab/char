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

const HIGHLIGHT_DURATION: u64 = 150;
const SCROLL_OFFSET: usize = 10;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ratatui::run(|terminal| App::default().run(terminal))?;
    Ok(())
}

#[derive(Default)]
pub struct App {
    lines_in_view: [usize; 2],
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
    selections: Vec<VisualSelection>,
    yank_buffer: HashMap<char, YankBuffer>,
    highlight_yank: bool,
    query: String,
    visual_block_rng: Option<VisualBlockRng>,
}

#[derive(Clone)]
enum YankBuffer {
    Chars(String),
    Lines(String),
    Block(Vec<String>),
}

#[derive(Default, Debug)]
struct VisualBlockRng {
    x_rng: [usize; 2],
    y_rng: [usize; 2],
}

#[derive(Default, Debug, PartialEq)]
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
    VisualLine(usize),
    VisualBlock,
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
        let mut visual_block_rng = None;
        let mut highlight_text = false;
        if self.highlight_yank {
            self.redraw = true;
            highlight_text = true;
        }
        match self.mode {
            Mode::Command => {
                self.cursor_pos.y = self.main_height + 2;
                self.cursor_pos.x = self.command_bar.len();
            }
            Mode::Search => {
                self.cursor_pos.y = self.main_height + 2;
                self.cursor_pos.x = self.command_bar.len();
                highlight_text = true;
            }
            Mode::Visual | Mode::VisualLine(_) => {
                highlight_text = true;
            }
            Mode::VisualBlock => {
                highlight_text = true;
                if let Some(rng) = &mut self.visual_block_rng {
                    rng.x_rng[1] = self.cursor_pos.x;
                    rng.y_rng[1] = self.cursor_pos.y;
                    let mut y_rng = rng.y_rng.clone();
                    let mut x_rng = rng.x_rng.clone();
                    y_rng.sort();
                    x_rng.sort();
                    visual_block_rng = Some(VisualBlockRng { x_rng, y_rng });
                }
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
        self.lines_in_view = [start_line_idx, end_line_idx];

        // convert rope slice to ratatui line
        let mut lines = Vec::new();
        let mut line_nums = vec![];
        for line_num in start_line_idx..end_line_idx {
            if let Some(rope_line) = self.rope.get_line(line_num as usize) {
                let line_length = rope_line.len_chars();
                let line_start_char = self.rope.line_to_char(line_num);
                let line_end_char = line_start_char + line_length;

                let mut current_selections = vec![];
                if highlight_text {
                    for sel in &self.selections {
                        let start = sel.ancor.min(sel.cursor);
                        let end = sel.ancor.max(sel.cursor);
                        if highlight_text && line_end_char > start && line_start_char <= end {
                            current_selections.push([start, end]);
                        }
                    }
                }
                if !current_selections.is_empty() || visual_block_rng.is_some() {
                    let mut line_of_spans = vec![];
                    let mut char_buffer = String::new();
                    let mut highlighting = false;
                    for (char_idx, c) in rope_line.chars().enumerate() {
                        let abs_idx = line_start_char + char_idx;
                        let mut in_select_rng = false;
                        for rng in &current_selections {
                            if abs_idx >= rng[0] && abs_idx <= rng[1] {
                                in_select_rng = true;
                                break;
                            }
                        }
                        if !in_select_rng {
                            if let Some(rng) = &visual_block_rng {
                                let y_rng = rng.y_rng[0]..=rng.y_rng[1];
                                let x_rng = rng.x_rng[0]..=rng.x_rng[1];
                                if y_rng.contains(&line_num) && x_rng.contains(&char_idx) {
                                    in_select_rng = true;
                                }
                            }
                        }
                        if in_select_rng {
                            if line_length == 1 && c == '\n' {
                                line_of_spans
                                    .push(Span::raw(" ").fg(Color::White).bg(Color::DarkGray));
                                continue;
                            }
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
            match event::poll(std::time::Duration::from_millis(HIGHLIGHT_DURATION)) {
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
            Mode::Command | Mode::Search => {
                match key_event.code {
                    KeyCode::Enter => {
                        match self.command_bar.as_str() {
                            ":q" => {
                                self.exit();
                                return;
                            }
                            _ => {}
                        }
                        if self.mode == Mode::Search {
                            self.query = extract_query(&self.command_bar);
                            let char_idx = self.rope.line_to_char(self.cursor_pos.preferred_y)
                                + self.cursor_pos.preferred_x;
                            if let Some(idx) =
                                next_search_result_idx(char_idx, &self.query, &self.rope, None)
                            {
                                let cursor_target_idx = idx;
                                self.update_cursor_from_char_idx(cursor_target_idx);
                                self.cursor_pos.preferred_y = self.cursor_pos.y;
                                self.cursor_pos.preferred_x = self.cursor_pos.x;
                            }
                        }
                        self.cursor_pos.y = self.cursor_pos.preferred_y;
                        self.cursor_pos.x = self.cursor_pos.preferred_x;
                        self.return_to_normal_mode();
                        self.scroll(self.cursor_pos.y);

                        // replace all
                        if self.command_bar.starts_with(":%s/") {
                            let replacment = extract_replacment(&self.command_bar);
                            if self.query.is_empty() || replacment.is_empty() {
                                return;
                            }

                            let char_idx = self.rope.line_to_char(self.cursor_pos.preferred_y)
                                + self.cursor_pos.preferred_x;
                            while let Some(idx) =
                                next_search_result_idx(char_idx, &self.query, &self.rope, None)
                            {
                                // replace text
                                self.rope.remove(idx..idx + self.query.len());
                                self.rope.insert(idx, &replacment);
                            }
                        }

                        return;
                    }
                    KeyCode::Char(c) => {
                        self.command_bar.push(c);
                    }
                    KeyCode::Backspace => {
                        self.command_bar.pop();
                        if self.command_bar.is_empty() {
                            self.cursor_pos.y = self.cursor_pos.preferred_y;
                            self.cursor_pos.x = self.cursor_pos.preferred_x;
                            self.return_to_normal_mode();
                        }
                    }
                    _ => {}
                }

                let mut highlight_all = false;
                if self.command_bar.starts_with(":%s/") {
                    self.mode = Mode::Search;
                    highlight_all = true;
                }
                if self.mode == Mode::Search {
                    self.selections.clear();
                    self.query = extract_query(&self.command_bar);
                    if self.query.is_empty() {
                        self.scroll(self.cursor_pos.preferred_y);
                        return;
                    }
                    if highlight_all {
                        let mut idx = self.rope.line_to_char(self.cursor_pos.preferred_y)
                            + self.cursor_pos.preferred_x;
                        while let Some(i) = next_search_result_idx(
                            idx,
                            &self.query,
                            &self.rope,
                            Some(self.lines_in_view),
                        ) {
                            idx = i;
                            let sel = VisualSelection {
                                ancor: idx,
                                cursor: idx + self.query.len() - 1,
                            };
                            if self.selections.contains(&sel) {
                                let target_y = self.rope.char_to_line(idx);
                                self.scroll(target_y);
                                break;
                            }
                            self.selections.push(sel);
                        }
                    } else {
                        let char_idx = self.rope.line_to_char(self.cursor_pos.preferred_y)
                            + self.cursor_pos.preferred_x;
                        if let Some(idx) =
                            next_search_result_idx(char_idx, &self.query, &self.rope, None)
                        {
                            let sel = VisualSelection {
                                ancor: idx,
                                cursor: idx + self.query.len() - 1,
                            };
                            self.selections.push(sel);
                            let target_y = self.rope.char_to_line(idx);
                            self.scroll(target_y);
                        }
                    }
                }
            }
            Mode::Insert => self.insert_text(key_event),
            _ => {
                let visual_mode = match self.mode {
                    Mode::Visual => true,
                    Mode::VisualLine(_) => true,
                    Mode::VisualBlock => true,
                    _ => false,
                };
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
        let mut should_update_preferred_x = false;
        let mut should_move_cursor = true;
        let mut should_save_command = false;
        let char_idx = self.get_char_idx();
        let mut range = (char_idx, char_idx);
        let mut cursor_target_idx = char_idx;
        let mut count = 1;
        if let Ok(n) = command.count.parse::<usize>() {
            count = n;
        }

        // check for motion
        match (command.motion, command.action, command.modifier) {
            (Some(Motion::EnterSearchMode), _, _) => {
                self.cursor_pos.preferred_y = self.cursor_pos.y;
                self.cursor_pos.preferred_x = self.cursor_pos.x;
                self.change_mode(Mode::Search);
                return;
            }
            (Some(Motion::EnterCommandMode), _, _) => {
                self.cursor_pos.preferred_y = self.cursor_pos.y;
                self.cursor_pos.preferred_x = self.cursor_pos.x;
                self.change_mode(Mode::Command);
                return;
            }
            (Some(Motion::FileStart), _, _) => {
                range = (0, char_idx);
                cursor_target_idx = 0;
                should_update_preferred_x = true;
            }
            (Some(Motion::VisualMode), _, _) => {
                self.selections.clear();
                self.change_mode(Mode::Visual);
                return;
            }
            (Some(Motion::VisualLineMode), _, _) => {
                let y = self.cursor_pos.y;
                let new_selection = VisualSelection {
                    ancor: self.rope.line_to_char(y),
                    cursor: line_end_idx(char_idx, &self.rope),
                };
                self.selections.clear();
                self.selections.push(new_selection);
                self.change_mode(Mode::VisualLine(y));
                return;
            }
            (Some(Motion::VisualBlockMode), _, _) => {
                self.selections.clear();

                let x = self.cursor_pos.x;
                let y = self.cursor_pos.y;

                let visual_block_rng = VisualBlockRng {
                    x_rng: [x, x],
                    y_rng: [y, y],
                };

                self.visual_block_rng = Some(visual_block_rng);
                self.change_mode(Mode::VisualBlock);
                return;
            }
            (Some(Motion::InsertMode), _, _) => {
                should_save_command = true;
                self.change_mode(Mode::Insert);
                should_move_cursor = false;
            }
            (Some(Motion::UpperInsert), _, _) => {
                should_save_command = true;
                cursor_target_idx = first_word_idx(&self.cursor_pos, &self.rope);
                self.update_cursor_from_char_idx(cursor_target_idx);
                self.change_mode(Mode::Insert);
                should_move_cursor = false;
            }
            (Some(Motion::Append), _, _) => {
                should_save_command = true;
                self.cursor_pos.x += 1;
                self.change_mode(Mode::Insert);
                should_move_cursor = false;
            }
            (Some(Motion::UpperAppend), _, _) => {
                should_save_command = true;
                let rope_line = self.rope.line(self.cursor_pos.y);
                if is_empty_line(&rope_line) {
                    self.change_mode(Mode::Insert);
                }
                cursor_target_idx = line_end_idx(char_idx, &self.rope);
                self.update_cursor_from_char_idx(cursor_target_idx);
                self.change_mode(Mode::Insert);
                should_move_cursor = false;
            }
            (Some(Motion::Left), _, _) => {
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
            (Some(Motion::Right), _, _) => {
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
            (Some(Motion::Up), Some(action), _) => match action {
                Action::Change => {
                    let start = self
                        .rope
                        .line_to_char(self.cursor_pos.y.saturating_sub(count));
                    let end = self
                        .rope
                        .line_to_char(self.cursor_pos.y + 1)
                        .saturating_sub(1);
                    range = (start, end);
                }
                _ => {
                    let start = self
                        .rope
                        .line_to_char(self.cursor_pos.y.saturating_sub(count));
                    let end = self.rope.line_to_char(self.cursor_pos.y + 1);
                    range = (start, end);
                }
            },
            (Some(Motion::Up), None, _) => {
                self.cursor_pos.x = self.cursor_pos.preferred_x;
                range = (char_idx, cursor_up_idx(&self.cursor_pos, count, &self.rope));
                cursor_target_idx = range.1;
            }
            (Some(Motion::Down), Some(action), _) => match action {
                Action::Change => {
                    let start = self.rope.line_to_char(self.cursor_pos.y);
                    let end = self.rope.line_to_char(self.cursor_pos.y + count);
                    range = (start, end);
                }
                _ => {
                    let start = self.rope.line_to_char(self.cursor_pos.y);
                    let end = self.rope.line_to_char(self.cursor_pos.y + count + 1);
                    range = (start, end);
                }
            },
            (Some(Motion::Down), None, _) => {
                self.cursor_pos.x = self.cursor_pos.preferred_x;
                range = (
                    char_idx,
                    cursor_down_idx(&self.cursor_pos, count, &self.rope),
                );
                cursor_target_idx = range.1;
            }
            (Some(Motion::HalfScreenUp), _, _) => {
                self.cursor_pos.x = self.cursor_pos.preferred_x;
                range = (
                    char_idx,
                    cursor_up_idx(&self.cursor_pos, self.main_height / 2, &self.rope),
                );
                cursor_target_idx = range.1;
            }
            (Some(Motion::HalfScreenDown), _, _) => {
                self.cursor_pos.x = self.cursor_pos.preferred_x;
                range = (
                    char_idx,
                    cursor_down_idx(&self.cursor_pos, self.main_height / 2, &self.rope),
                );
                cursor_target_idx = range.1;
            }
            (Some(Motion::NextEmptyLine), _, _) => {
                for _ in 0..count {
                    range = (char_idx, next_empty_line_idx(range.1, &self.rope));
                }
                cursor_target_idx = range.1;
            }
            (Some(Motion::PrevEmptyLine), _, _) => {
                for _ in 0..count {
                    range = (prev_empty_line_idx(range.0, &self.rope), char_idx);
                }
                cursor_target_idx = range.0;
            }
            (Some(Motion::Percent), _, _) => {
                if let Some(i) = matching_bracket_idx(&self.cursor_pos, char_idx, &self.rope) {
                    range.1 = i;
                    cursor_target_idx = range.1;
                    should_update_preferred_x = true;
                } else {
                    return;
                }
            }
            (Some(Motion::Backtick), _, Some(modifier)) => {
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
            (Some(Motion::SingleQuote), _, Some(modifier)) => {
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
            (Some(Motion::DoubleQuote), _, Some(modifier)) => {
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
            (Some(Motion::OpenAngleBracket), _, Some(modifier)) => {
                if let Some(r) = inside_brackets(char_idx, &self.rope, '<', '>') {
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
            (Some(Motion::OpenCurlyBrace), _, Some(modifier)) => {
                if let Some(r) = inside_brackets(char_idx, &self.rope, '{', '}') {
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
            (Some(Motion::OpenBracket), _, Some(modifier)) => {
                if let Some(r) = inside_brackets(char_idx, &self.rope, '[', ']') {
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
            (Some(Motion::OpenParen), _, Some(modifier)) => {
                if let Some(r) = inside_brackets(char_idx, &self.rope, '(', ')') {
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
            (Some(Motion::Word), _, Some(commands::Modifier::Inside)) => {
                let rope_line = self.rope.line(self.cursor_pos.y);
                if is_empty_line(&rope_line) {
                    self.last_command = command.clone();
                    return;
                }
                range = inside_word(char_idx, &self.rope);
                cursor_target_idx = range.0;
                should_update_preferred_x = true;
            }
            (Some(Motion::Word), _, None) => {
                // delete, change, and yank should stop at \n
                for _ in 0..count {
                    range = (char_idx, next_word_idx(range.1, &self.rope, action));
                }
                cursor_target_idx = range.1;
                should_update_preferred_x = true;
            }
            (Some(Motion::UpperWord), _, Some(commands::Modifier::Inside)) => {
                let rope_line = self.rope.line(self.cursor_pos.y);
                if is_empty_line(&rope_line) {
                    self.last_command = command.clone();
                    return;
                }
                range = inside_upper_word(char_idx, &self.rope);
                cursor_target_idx = range.0;
                should_update_preferred_x = true;
            }
            (Some(Motion::UpperWord), _, _) => {
                for _ in 0..count {
                    range = (char_idx, upper_word_idx(range.1, &self.rope, action));
                }
                cursor_target_idx = range.1;
                should_update_preferred_x = true;
            }
            (Some(Motion::End), _, _) => {
                let mut range_end = char_idx;
                for _ in 0..count {
                    range_end = word_end_idx(range_end, &self.rope);
                }
                range = (char_idx, range_end + 1);
                cursor_target_idx = range_end;
                should_update_preferred_x = true;
            }
            (Some(Motion::UpperEnd), _, _) => {
                for _ in 0..count {
                    range = (char_idx, upper_word_end_idx(range.1, &self.rope));
                }
                cursor_target_idx = range.1;
                should_update_preferred_x = true;
            }
            (Some(Motion::Back), _, _) => {
                for _ in 0..count {
                    range = (prev_word_idx(range.0, &self.rope), char_idx);
                }
                cursor_target_idx = range.0;
                should_update_preferred_x = true;
            }
            (Some(Motion::UpperBack), _, _) => {
                for _ in 0..count {
                    range = (upper_back_word_idx(range.0, &self.rope), char_idx);
                }
                cursor_target_idx = range.0;
                should_update_preferred_x = true;
            }
            (Some(Motion::FirstWord), _, _) => {
                cursor_target_idx = first_word_idx(&self.cursor_pos, &self.rope);
                range = (
                    char_idx.min(cursor_target_idx),
                    char_idx.max(cursor_target_idx),
                );
                should_update_preferred_x = true;
            }
            (Some(Motion::LineStart), _, _) => {
                range = (line_start_idx(self.cursor_pos.y, &self.rope), char_idx);
                cursor_target_idx = range.0;
                should_update_preferred_x = true;
            }
            (Some(Motion::LineEnd), _, _) => {
                let rope_line = self.rope.line(self.cursor_pos.y);
                if is_empty_line(&rope_line) {
                    return;
                }
                cursor_target_idx = line_end_idx(char_idx, &self.rope);
                range = (char_idx, cursor_target_idx);
                self.cursor_pos.preferred_x = usize::MAX;
            }
            (Some(Motion::FileEnd), _, _) => {
                range = (char_idx, file_end_idx(&self.rope));
                cursor_target_idx = range.1;
                should_update_preferred_x = true;
            }
            (Some(Motion::NewLineBelow), _, _) => {
                should_save_command = true;
                self.change_mode(Mode::Insert);
                should_move_cursor = false;
                // auto indent
                // respect previous line whitespace
                // if inside pair, add tab

                let y = self.cursor_pos.y;
                let mut text = String::from('\n');
                let opening_brackets = ['[', '(', '{', '<'];
                let last_char_idx = line_end_idx(char_idx, &self.rope);
                let last_char = self.rope.char(last_char_idx.saturating_sub(1));

                if opening_brackets.contains(&last_char) {
                    // get whitespace of current line
                    let curr_line = self.rope.line(y);
                    let whitespace: String = curr_line
                        .chars()
                        .take_while(|c| c.is_whitespace() && *c != '\n' && *c != '\r')
                        .collect();
                    // add tab & new line
                    text.push_str(&whitespace);
                    text.push_str("    ");
                    self.rope.insert(last_char_idx, &text);
                    self.last_insertion += "\n";
                    let cursor_target_idx = last_char_idx + whitespace.len() + 5;
                    self.update_cursor_from_char_idx(cursor_target_idx);
                } else {
                    let (insert_pos, whitespace) = new_line_below_idx(&self.cursor_pos, &self.rope);
                    self.rope.insert(insert_pos, &format!("\n{}", whitespace));
                    self.cursor_pos.y += 1;
                    self.cursor_pos.x = whitespace.chars().count();
                }
            }
            (Some(Motion::NewLineAbove), _, _) => {
                should_save_command = true;
                let (insert_pos, whitespace) = new_line_above_idx(&self.cursor_pos, &self.rope);
                let insert_str = format!("{}\n", whitespace);
                self.rope.insert(insert_pos, &insert_str);
                self.cursor_pos.x = whitespace.chars().count();
                self.change_mode(Mode::Insert);
                should_move_cursor = false;
            }
            (Some(Motion::DeleteLine) | Some(Motion::YankLine), _, _) => {
                should_save_command = true;
                range = (
                    self.rope.line_to_char(self.cursor_pos.y),
                    self.rope.line_to_char(self.cursor_pos.y + count),
                );
            }
            (Some(Motion::ChangeLine), _, _) => {
                should_save_command = true;
                range = (
                    self.rope.line_to_char(self.cursor_pos.y),
                    self.rope.line_to_char(self.cursor_pos.y + count) - 1,
                );
            }
            (Some(Motion::UpperChange), _, _) => {
                let rope_line = self.rope.line(self.cursor_pos.y);
                if is_empty_line(&rope_line) {
                    self.change_mode(Mode::Insert);
                    return;
                }
                range = (char_idx, line_end_idx(char_idx, &self.rope));
                should_move_cursor = false;
            }
            (Some(Motion::Paste), _, _) => {
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
                        YankBuffer::Block(strings) => {
                            let mut y = self.cursor_pos.y;
                            let x = self.cursor_pos.x;
                            for s in strings {
                                let mut insert_idx = self.rope.line_to_char(y) + x;
                                // if on empty line, insert before cursor
                                if self.rope.char(insert_idx) != '\n' {
                                    insert_idx += 1;
                                }
                                self.rope.insert(insert_idx, &s);
                                cursor_target_idx = char_idx + s.len();
                                y += 1;
                            }
                        }
                    }
                }
            }
            (Some(Motion::UpperPaste), _, _) => {
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
                            let idx = self.rope.line_to_char(self.cursor_pos.y);
                            self.rope.insert(idx, &content);
                            cursor_target_idx = idx;
                        }
                        YankBuffer::Block(strings) => {
                            let mut y = self.cursor_pos.y;
                            let x = self.cursor_pos.x;
                            for s in strings {
                                let mut insert_idx = self.rope.line_to_char(y) + x;
                                self.rope.insert(insert_idx, &s);
                                cursor_target_idx = char_idx + s.len();
                                y += 1;
                            }
                        }
                    }
                }
            }
            (Some(Motion::NextSearchResult), _, _) => {
                if let Some(idx) = next_search_result_idx(char_idx, &self.query, &self.rope, None) {
                    cursor_target_idx = idx;
                    should_update_preferred_x = true;
                } else {
                    return;
                }
            }
            (Some(Motion::PrevSearchResult), _, _) => {
                if let Some(idx) = prev_search_result_idx(char_idx, &self.query, &self.rope) {
                    cursor_target_idx = idx;
                    should_update_preferred_x = true;
                } else {
                    return;
                }
            }
            (Some(Motion::Repeat), _, _) => {
                self.execute_command(self.last_command.clone(), visual_mode, true);
                if self.mode == Mode::Insert {
                    let idx = self.get_char_idx();
                    self.rope.insert(idx, &self.last_insertion);
                    self.update_cursor_from_char_idx(idx + self.last_insertion.len() - 1);
                    self.ensure_valid_normal_pos();
                }
                self.change_mode(Mode::Normal);
                return;
            }
            (Some(Motion::Star), _, _) => {
                let word_range = inside_word(char_idx, &self.rope);
                let word = self.rope.slice(word_range.0..word_range.1);
                self.query = word.to_string();
                self.command_bar.clear();
                self.command_bar.push('/');
                self.command_bar.push_str(&self.query);
                if let Some(idx) = next_search_result_idx(char_idx, &self.query, &self.rope, None) {
                    cursor_target_idx = idx;
                    should_update_preferred_x = true;
                } else {
                    return;
                }
            }
            (
                None,
                _,
                Some(commands::Modifier::Find {
                    c,
                    forwards,
                    inclusive,
                }),
            ) => {
                if let Some(idx) =
                    find_char_inline(&self.cursor_pos, &self.rope, c, forwards, inclusive)
                {
                    let start = char_idx.min(idx);
                    let mut end = char_idx.max(idx);
                    if forwards {
                        end += 1;
                    }
                    range = (start, end);
                    cursor_target_idx = idx;
                } else {
                    return;
                }
            }
            (Some(Motion::Substitute), Some(_action), None) => {
                range = (char_idx, char_idx + count);
            }
            _ => {}
        }

        // update selection range
        match self.mode {
            Mode::VisualLine(y) => {
                // if cursor is after ancor, ancor is at start of line
                // else ancor is at end of line
                if let Some(sel) = self.selections.first_mut() {
                    if cursor_target_idx >= sel.ancor {
                        sel.ancor = self.rope.line_to_char(y);
                        sel.cursor = line_end_idx(cursor_target_idx, &self.rope);
                    } else {
                        sel.ancor = line_end_idx(self.rope.line_to_char(y), &self.rope);
                        let curr_line = self.rope.char_to_line(cursor_target_idx);
                        sel.cursor = self.rope.line_to_char(curr_line);
                    }
                }
            }
            Mode::Visual => {
                if let Some(sel) = self.selections.first_mut() {
                    sel.cursor = cursor_target_idx;
                }
            }
            _ => {}
        }

        // update char range
        if visual_mode {
            should_save_command = false;
            if let Some(sel) = self.selections.first_mut() {
                let start_select_rng = sel.ancor.min(sel.cursor);
                let mut end_select_rng = sel.ancor.max(sel.cursor);
                match command.action {
                    Some(Action::Delete) => end_select_rng += 1,
                    Some(Action::Yank) => end_select_rng += 1,
                    _ => {}
                }
                range = (start_select_rng, end_select_rng);
            }
        }

        // check for yank
        match command.action {
            Some(Action::Yank) | Some(Action::Delete) | Some(Action::Change) => {
                if self.mode == Mode::VisualBlock {
                    if let Some(rng) = &self.visual_block_rng {
                        let mut x_rng = rng.x_rng.clone();
                        x_rng.sort();
                        let mut y_rng = rng.y_rng.clone();
                        y_rng.sort();

                        let mut slices = vec![];
                        for y in y_rng[0]..y_rng[1] {
                            let line_char = self.rope.line_to_char(y);
                            let start = line_char + x_rng[0];
                            let end = line_char + x_rng[1];
                            if let Some(slice) = self.rope.get_slice(start..=end) {
                                slices.push(slice.to_string());
                            }
                        }
                        let buf = YankBuffer::Block(slices);
                        self.yank_buffer
                            .entry('"')
                            .and_modify(|content| *content = buf.clone())
                            .or_insert(buf);
                    }
                } else if let Some(slice) = self.rope.get_slice(range.0..range.1) {
                    let mut yank_lines = false;
                    let mut count = 0;
                    for c in slice.chars() {
                        if c == '\n' {
                            count += 1;
                        }
                        if count > 1 {
                            yank_lines = true;
                            break;
                        }
                    }
                    let new_content = if yank_lines {
                        YankBuffer::Lines(String::from(slice))
                    } else {
                        YankBuffer::Chars(String::from(slice))
                    };
                    self.yank_buffer
                        .entry('"')
                        .and_modify(|content| *content = new_content.clone())
                        .or_insert(new_content);
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
            }
            Some(Action::Delete) | Some(Action::Change) => {
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
        }

        if should_update_preferred_x {
            self.cursor_pos.preferred_x = self.cursor_pos.x;
        }

        self.scroll(self.cursor_pos.y);
    }

    fn insert_text(&mut self, e: KeyEvent) {
        let mut text_to_insert = None;
        let idx = self.get_char_idx();
        match e.code {
            KeyCode::Char(c) => {
                let pairs = [
                    ['[', ']'],
                    ['{', '}'],
                    ['(', ')'],
                    ['<', '>'],
                    ['"', '"'],
                    ['\'', '\''],
                    ['`', '`'],
                ];
                let mut text = String::from(c);
                if let Some(pair) = pairs.iter().find(|e| e.contains(&c)) {
                    if pair[0] == c {
                        text.push(pair[1]);
                    } else {
                        if self.rope.char(idx) == c {
                            text.clear();
                        }
                    }
                }
                if !text.is_empty() {
                    text_to_insert = Some(text);
                }
                self.cursor_pos.x += 1;
            }
            KeyCode::Tab => {
                text_to_insert = Some(String::from("    "));
                self.cursor_pos.x += 4;
            }
            KeyCode::Backspace => {
                let x = self.cursor_pos.x;
                let y = self.cursor_pos.y;
                self.last_insertion.pop();

                if x > 0 {
                    // NORMAL BACKSPACE: Just delete the char to the left
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
                // auto indent
                // respect previous line whitespace
                // if inside pair, add tab

                let y = self.cursor_pos.y;
                let mut text = String::from('\n');
                let opening_brackets = ['[', '(', '{', '<'];
                let closing_brackets = [']', ')', '}', '>'];
                let left_char = self.rope.char(idx.saturating_sub(1));
                let c = self.rope.char(idx);

                if opening_brackets.contains(&left_char) && closing_brackets.contains(&c) {
                    // get whitespace of current line
                    let curr_line = self.rope.line(y);
                    let whitespace: String = curr_line
                        .chars()
                        .take_while(|c| c.is_whitespace() && *c != '\n' && *c != '\r')
                        .collect();
                    // add tab & new line
                    text.push_str(&whitespace);
                    text.push_str("    \n");
                    // add whitespace again
                    text.push_str(&whitespace);
                    // move the cursor forwards whitepace + '\n'
                    self.rope.insert(idx, &text);
                    self.last_insertion += "\n";
                    let cursor_target_idx = idx + whitespace.len() + 5;
                    self.update_cursor_from_char_idx(cursor_target_idx);
                } else if opening_brackets.contains(&left_char) {
                    // get whitespace of current line
                    let curr_line = self.rope.line(y);
                    let whitespace: String = curr_line
                        .chars()
                        .take_while(|c| c.is_whitespace() && *c != '\n' && *c != '\r')
                        .collect();
                    // add tab & new line
                    text.push_str(&whitespace);
                    text.push_str("    ");
                    self.rope.insert(idx, &text);
                    self.last_insertion += "\n";
                    let cursor_target_idx = idx + whitespace.len() + 5;
                    self.update_cursor_from_char_idx(cursor_target_idx);
                } else {
                    // get whitespace of current line
                    let curr_line = self.rope.line(y);
                    let whitespace: String = curr_line
                        .chars()
                        .take_while(|c| c.is_whitespace() && *c != '\n' && *c != '\r')
                        .collect();
                    text.push_str(&whitespace);
                    text_to_insert = Some(text);
                    self.cursor_pos.y += 1;
                    self.cursor_pos.x = whitespace.len();
                }
            }
            _ => {}
        }
        if let Some(text) = text_to_insert {
            self.rope.insert(idx, &text);
            self.last_insertion += &text;
        }
        self.scroll(self.cursor_pos.y);
    }

    fn scroll(&mut self, target_y: usize) {
        let offset = SCROLL_OFFSET;
        let height = self.main_height - 1 - offset;
        // don't let cursor go beyond file length
        // self.cursor_pos.y = target_y.min(self.rope.len_lines().saturating_sub(2));

        if target_y.saturating_sub(self.top_line) >= height {
            // scroll down
            self.top_line = target_y.saturating_sub(height);
        } else if target_y <= self.top_line + offset {
            // scroll up
            self.top_line = target_y.saturating_sub(offset);
        }
    }

    fn return_to_normal_mode(&mut self) {
        self.change_mode(Mode::Normal);
        self.parser.reset();
        self.ensure_valid_normal_pos();
        self.cursor_pos.preferred_x = self.cursor_pos.x;
        self.scroll(self.cursor_pos.y);
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
                self.selections.clear();
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
            Mode::VisualLine(_) => {
                self.command_bar.clear();
                self.command_bar.push_str("-- VISUAL LINE --");
            }
            Mode::VisualBlock => {
                self.command_bar.clear();
                self.command_bar.push_str("-- VISUAL BLOCK --");
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

    fn get_char_idx(&self) -> usize {
        self.rope.line_to_char(self.cursor_pos.y) + self.cursor_pos.x
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

fn next_search_result_idx(
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

fn prev_search_result_idx(char_idx: usize, query: &str, rope: &Rope) -> Option<usize> {
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

fn inside_brackets(
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

fn matching_bracket_idx(cursor_pos: &CursorPos, char_idx: usize, rope: &Rope) -> Option<usize> {
    let c = rope.char(char_idx);
    let opening_brackets = ['[', '(', '{', '<'];
    let closing_brackets = [']', ')', '}', '>'];

    // if cursor is on bracket
    if opening_brackets.contains(&c) || closing_brackets.contains(&c) {
        return find_matching_bracket(char_idx, rope, c);
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
        if opening_brackets.contains(&c) {
            if count == 0 {
                return Some(line_char_idx + idx);
            }
            count -= 1;
        } else if closing_brackets.contains(&c) {
            count += 1;
        }
    }

    // if outside brackets, find next bracket pair
    let mut found_opening = false;
    idx = x + 1;
    while idx < line.len_chars() - 1 {
        let c = line.char(idx);
        if opening_brackets.contains(&c) {
            found_opening = true;
        } else if closing_brackets.contains(&c) && found_opening {
            return Some(line_char_idx + idx);
        }
        idx += 1;
    }

    None
}

fn find_matching_bracket(char_idx: usize, rope: &Rope, token: char) -> Option<usize> {
    let mut idx = char_idx;
    let opening;
    let closing;
    match token {
        '[' | ']' => {
            opening = '[';
            closing = ']';
        }
        '(' | ')' => {
            opening = '(';
            closing = ')';
        }
        '{' | '}' => {
            opening = '{';
            closing = '}';
        }
        '<' | '>' => {
            opening = '<';
            closing = '>';
        }
        _ => {
            return None;
        }
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

fn find_char_inline(
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

fn extract_query(s: &str) -> String {
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

fn extract_replacment(s: &str) -> String {
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
