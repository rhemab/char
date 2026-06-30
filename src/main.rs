use std::{env, fs, io};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout},
    prelude::*,
    style::{Color, Style},
    widgets::{Block, Paragraph},
};

use ropey::Rope;
use syntect::{
    easy::HighlightLines,
    highlighting::{Style as SyntectStyle, ThemeSet},
    parsing::SyntaxSet,
};

use crate::command_parser::*;
use crate::helpers::*;
use crate::ranges::*;
use crate::types::*;

mod command_parser;
mod helpers;
mod ranges;
mod trie;
pub mod types;
mod undo;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const HIGHLIGHT_DURATION: u64 = 150;
const SCROLL_OFFSET: usize = 10;

const NEW_PATH: &'static str = "[new]";

const OPENING_BRACKETS: [char; 4] = ['[', '(', '{', '<'];
const CLOSING_BRACKETS: [char; 4] = [']', ')', '}', '>'];
const PAIRS: [[char; 2]; 7] = [
    ['[', ']'],
    ['{', '}'],
    ['(', ')'],
    ['<', '>'],
    ['"', '"'],
    ['\'', '\''],
    ['`', '`'],
];

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ratatui::run(|terminal| App::default().run(terminal))?;
    Ok(())
}

impl App {
    fn save_file_extension(&mut self) {
        if let Some(ext) = std::path::Path::new(&self.path).extension() {
            if let Some(ext_str) = ext.to_str() {
                self.file_extension = ext_str.to_string();
            }
        }
    }
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
            // try to load file
            if let Ok(file) = fs::File::open(&path) {
                self.rope = Rope::from_reader(file)?;
            } else {
                // file doesn't exist
                self.rope = Rope::from_str("\n");
            }
            self.path = path;
        } else {
            // no path was specified
            self.show_first_time_popup = true;
            self.rope = Rope::from_str("\n");
            self.path = NEW_PATH.to_string();
        }
        self.command_bar.push_str(&format!(
            "\"{}\" {}L, {}",
            &self.path,
            self.rope.len_lines().saturating_sub(1),
            format_file_size(self.rope.len_bytes()),
        ));

        self.yank_buffer
            .insert('"', YankBuffer::Chars(String::new()));

        self.highlight.syntax_set = SyntaxSet::load_defaults_newlines();
        self.highlight.theme_set = ThemeSet::load_defaults();

        self.save_file_extension();

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

        let mut h = None;
        if let Some(syntax) = self
            .highlight
            .syntax_set
            .find_syntax_by_extension(&self.file_extension)
        {
            h = Some(HighlightLines::new(
                syntax,
                &self.highlight.theme_set.themes["base16-ocean.dark"],
            ));
        }

        // convert rope slice to ratatui line
        let mut lines = Vec::new();
        let mut line_nums = vec![];
        for line_num in start_line_idx..end_line_idx {
            if let Some(rope_line) = self.rope.get_line(line_num as usize) {
                let line_length = rope_line.len_chars();
                let line_start_char = self.rope.line_to_char(line_num);
                let line_end_char = line_start_char + line_length;

                let mut current_selections = vec![];

                // check for matching bracket
                if let Some(i) = self.matching_bracket_idx {
                    // only push for current line
                    if line_end_char > i && line_start_char <= i {
                        current_selections.push([i, i]);
                    }
                }

                if highlight_text {
                    for sel in &self.selections {
                        let start = sel.ancor.min(sel.cursor);
                        let end = sel.ancor.max(sel.cursor);
                        // check if line contains a selection
                        if line_end_char > start && line_start_char <= end {
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
                        // check visual block range
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
                                line_of_spans.push(Span::raw(std::mem::take(&mut char_buffer)));
                            }
                            highlighting = true;
                            char_buffer.push(c);
                        } else {
                            if highlighting && !char_buffer.is_empty() {
                                line_of_spans.push(
                                    Span::raw(std::mem::take(&mut char_buffer))
                                        .fg(Color::White)
                                        .bg(Color::DarkGray),
                                );
                            }
                            highlighting = false;
                            char_buffer.push(c);
                        }
                    }
                    if !char_buffer.is_empty() {
                        if highlighting {
                            line_of_spans.push(
                                Span::raw(std::mem::take(&mut char_buffer))
                                    .fg(Color::White)
                                    .bg(Color::DarkGray),
                            );
                        } else {
                            line_of_spans.push(Span::raw(std::mem::take(&mut char_buffer)));
                        }
                    }

                    lines.push(Line::from(line_of_spans));
                } else {
                    let line_str = rope_line.to_string();
                    if let Some(hi) = &mut h {
                        let ranges: Vec<(SyntectStyle, &str)> = hi
                            .highlight_line(&line_str, &self.highlight.syntax_set)
                            .unwrap();
                        let mut spans = vec![];
                        for (syntect_style, content) in ranges {
                            let r = syntect_style.foreground.r;
                            let g = syntect_style.foreground.g;
                            let b = syntect_style.foreground.b;
                            let style = Style::new().fg(Color::Rgb(r, g, b));
                            let span = Span::styled(content.to_string(), style);
                            spans.push(span);
                        }
                        lines.push(Line::from(spans));
                    } else {
                        lines.push(Line::from(line_str));
                    }
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

        if self.show_first_time_popup && self.rope.len_chars() > 1 {
            self.show_first_time_popup = false;
        }
        if self.show_first_time_popup {
            // render first time popup
            let area = frame
                .area()
                .centered(Constraint::Percentage(40), Constraint::Percentage(40));

            let lines = vec![
                Line::from(Span::styled(
                    "char",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "fast, reliable, zero config",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled(":w <file>", Style::default().fg(Color::Magenta)),
                    Span::raw("     write a file"),
                ]),
                Line::from(vec![
                    Span::styled(":q", Style::default().fg(Color::Magenta)),
                    Span::raw("            quit"),
                ]),
                Line::from(vec![
                    Span::styled(":wq", Style::default().fg(Color::Magenta)),
                    Span::raw("           write and quit"),
                ]),
                Line::from(""),
                Line::from(Span::styled(VERSION, Style::default().fg(Color::DarkGray))),
            ];

            let para = Paragraph::new(lines);
            frame.render_widget(para, area);
        }
    }

    fn handle_events(&mut self) -> io::Result<()> {
        // if self.redraw is true and there is not event within the specified duration,
        // then redraw the screen.
        // This is used to show a yank highlight for the specified duration.
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
                        let command_bar = self.command_bar.clone();
                        let mut command_iter = command_bar.split_whitespace();
                        let command = command_iter.next();
                        let arg = command_iter.next();
                        match (command, arg) {
                            (Some(":q"), _) => {
                                self.exit();
                                return;
                            }
                            (Some(":w") | Some(":wq"), arg) => {
                                self.command_bar.clear();
                                // try save to path: arg
                                if let Some(path) = arg {
                                    match helpers::write_file(&self.rope, path) {
                                        Ok(s) => {
                                            self.command_bar.push_str(&s);
                                            if self.path == NEW_PATH {
                                                self.path = path.to_string();
                                            }
                                        }
                                        Err(err) => {
                                            self.command_bar.push_str(&format!("error: {:?}", err));
                                        }
                                    }
                                } else if self.path == NEW_PATH {
                                    self.command_bar
                                        .push_str(&format!("error: no file name specified"));
                                } else if !self.path.is_empty() {
                                    // save to self.path
                                    match helpers::write_file(&self.rope, &self.path) {
                                        Ok(s) => {
                                            self.command_bar.push_str(&s);
                                        }
                                        Err(err) => {
                                            self.command_bar.push_str(&format!("error: {:?}", err));
                                        }
                                    }
                                }

                                match command {
                                    Some(":wq") => {
                                        self.exit();
                                        return;
                                    }
                                    _ => {
                                        self.save_file_extension();
                                    }
                                }
                            }
                            (Some(":e") | Some(":edit"), arg) => {
                                if let Some(path) = arg {
                                    if let Ok(file) = fs::File::open(&path) {
                                        // add file to new rope
                                    } else {
                                        // create new file
                                    }
                                } else {
                                    // create new buffer
                                }
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

    fn execute_command(
        &mut self,
        command: command_parser::Command,
        visual_mode: bool,
        repeat: bool,
    ) {
        self.parser.reset();

        let action = command.action.is_some();
        let mut should_update_preferred_x = false;
        let mut should_update_preferred_y = false;
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
                should_update_preferred_y = true;
            }
            (Some(Motion::VisualMode), _, _) => {
                let new_selection = VisualSelection {
                    ancor: char_idx,
                    cursor: char_idx,
                };
                self.selections.clear();
                self.selections.push(new_selection);
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
                should_update_preferred_y = true;
            }
            (Some(Motion::Down), Some(action), _) => match action {
                Action::Change => {
                    let start = self.rope.line_to_char(self.cursor_pos.y);
                    let end_y = self.cursor_pos.y + count;
                    let end_line_len = self.rope.line(end_y).len_chars();
                    let end = self.rope.line_to_char(end_y) + end_line_len.saturating_sub(1);
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
                should_update_preferred_y = true;
            }
            (Some(Motion::HalfScreenUp), _, _) => {
                self.cursor_pos.x = self.cursor_pos.preferred_x;
                range = (
                    char_idx,
                    cursor_up_idx(&self.cursor_pos, self.main_height / 2, &self.rope),
                );
                cursor_target_idx = range.1;
                should_update_preferred_y = true;
            }
            (Some(Motion::HalfScreenDown), _, _) => {
                self.cursor_pos.x = self.cursor_pos.preferred_x;
                range = (
                    char_idx,
                    cursor_down_idx(&self.cursor_pos, self.main_height / 2, &self.rope),
                );
                cursor_target_idx = range.1;
                should_update_preferred_y = true;
            }
            (Some(Motion::NextEmptyLine), _, _) => {
                for _ in 0..count {
                    range = (char_idx, next_empty_line_idx(range.1, &self.rope));
                }
                cursor_target_idx = range.1;
                should_update_preferred_y = true;
            }
            (Some(Motion::PrevEmptyLine), _, _) => {
                for _ in 0..count {
                    range = (prev_empty_line_idx(range.0, &self.rope), char_idx);
                }
                cursor_target_idx = range.0;
                should_update_preferred_y = true;
            }
            (Some(Motion::Percent), _, _) => {
                if let Some(i) = matching_bracket_idx(&self.cursor_pos, char_idx, &self.rope) {
                    range.1 = i;
                    cursor_target_idx = range.1;
                    should_update_preferred_x = true;
                    should_update_preferred_y = true;
                } else {
                    return;
                }
            }
            (Some(Motion::Backtick), _, Some(modifier)) => {
                if let Some(r) =
                    inside_quotes(self.cursor_pos.x, self.cursor_pos.y, &self.rope, '`')
                {
                    match modifier {
                        command_parser::Modifier::Around => {
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
                        command_parser::Modifier::Around => {
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
                        command_parser::Modifier::Around => {
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
                        command_parser::Modifier::Around => {
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
                        command_parser::Modifier::Around => {
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
                        command_parser::Modifier::Around => {
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
                        command_parser::Modifier::Around => {
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
            (Some(Motion::Word), _, Some(command_parser::Modifier::Inside)) => {
                let rope_line = self.rope.line(self.cursor_pos.y);
                if is_empty_line(&rope_line) {
                    self.last_command = command.clone();
                    return;
                }
                range = inside_word(char_idx, &self.rope);
                cursor_target_idx = range.0;
                should_update_preferred_x = true;
                should_update_preferred_y = true;
            }
            (Some(Motion::Word), _, None) => {
                // delete, change, and yank should stop at \n
                for _ in 0..count {
                    range = (char_idx, next_word_idx(range.1, &self.rope, action));
                }
                cursor_target_idx = range.1;
                should_update_preferred_x = true;
                should_update_preferred_y = true;
            }
            (Some(Motion::UpperWord), _, Some(command_parser::Modifier::Inside)) => {
                let rope_line = self.rope.line(self.cursor_pos.y);
                if is_empty_line(&rope_line) {
                    self.last_command = command.clone();
                    return;
                }
                range = inside_upper_word(char_idx, &self.rope);
                cursor_target_idx = range.0;
                should_update_preferred_x = true;
                should_update_preferred_y = true;
            }
            (Some(Motion::UpperWord), _, _) => {
                for _ in 0..count {
                    range = (char_idx, upper_word_idx(range.1, &self.rope, action));
                }
                cursor_target_idx = range.1;
                should_update_preferred_x = true;
                should_update_preferred_y = true;
            }
            (Some(Motion::End), _, _) => {
                let mut range_end = char_idx;
                for _ in 0..count {
                    range_end = word_end_idx(range_end, &self.rope);
                }
                range = (char_idx, range_end + 1);
                cursor_target_idx = range_end;
                should_update_preferred_x = true;
                should_update_preferred_y = true;
            }
            (Some(Motion::UpperEnd), _, _) => {
                for _ in 0..count {
                    range = (char_idx, upper_word_end_idx(range.1, &self.rope));
                }
                cursor_target_idx = range.1;
                should_update_preferred_x = true;
                should_update_preferred_y = true;
            }
            (Some(Motion::Back), _, _) => {
                for _ in 0..count {
                    range = (prev_word_idx(range.0, &self.rope), char_idx);
                }
                cursor_target_idx = range.0;
                should_update_preferred_x = true;
                should_update_preferred_y = true;
            }
            (Some(Motion::UpperBack), _, _) => {
                for _ in 0..count {
                    range = (upper_back_word_idx(range.0, &self.rope), char_idx);
                }
                cursor_target_idx = range.0;
                should_update_preferred_x = true;
                should_update_preferred_y = true;
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
                should_update_preferred_y = true;
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
                let last_char_idx = line_end_idx(char_idx, &self.rope);
                let last_char = self.rope.char(last_char_idx.saturating_sub(1));

                if OPENING_BRACKETS.contains(&last_char) {
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
                    self.last_insertion.1 += "\n";
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
                if let Some(buf) = self.yank_buffer.get_mut(&'"') {
                    let mut new_content = String::new();
                    match buf {
                        YankBuffer::Chars(content) => {
                            let mut insert_idx = char_idx;
                            // if on empty line, insert before cursor
                            if self.rope.char(char_idx) != '\n' {
                                insert_idx += 1;
                            }
                            if visual_mode {
                                if let Some(sel) = self.selections.first() {
                                    let start = sel.ancor.min(sel.cursor);
                                    let end = sel.ancor.max(sel.cursor);
                                    new_content = self.rope.slice(start..=end).to_string();
                                    self.rope.remove(start..=end);
                                    insert_idx = start;
                                }
                            }
                            self.rope.insert(insert_idx, &content);
                            cursor_target_idx = (insert_idx + content.len()).saturating_sub(1);
                            self.selections.clear();
                            *content = new_content;
                            self.update_cursor_from_char_idx(cursor_target_idx);
                            self.return_to_normal_mode();
                            return;
                        }
                        YankBuffer::Lines(content) => {
                            // insert line below
                            let mut insert_idx = self.rope.line_to_char(self.cursor_pos.y + 1);
                            if visual_mode {
                                if let Some(sel) = self.selections.first() {
                                    let start = sel.ancor.min(sel.cursor);
                                    let end = sel.ancor.max(sel.cursor);
                                    self.rope.remove(start..=end);
                                    insert_idx = start;
                                }
                            }
                            self.rope.insert(insert_idx, &content);
                            cursor_target_idx = (insert_idx + content.len()).saturating_sub(1);
                            self.selections.clear();
                            self.update_cursor_from_char_idx(cursor_target_idx);
                            self.return_to_normal_mode();
                            return;
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
                                let insert_idx = self.rope.line_to_char(y) + x;
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
                    should_update_preferred_y = true;
                } else {
                    return;
                }
            }
            (Some(Motion::PrevSearchResult), _, _) => {
                if let Some(idx) = prev_search_result_idx(char_idx, &self.query, &self.rope) {
                    cursor_target_idx = idx;
                    should_update_preferred_x = true;
                    should_update_preferred_y = true;
                } else {
                    return;
                }
            }
            (Some(Motion::Repeat), _, _) => {
                self.execute_command(self.last_command.clone(), visual_mode, true);
                if self.mode == Mode::Insert {
                    let idx = self.get_char_idx();
                    self.rope.insert(idx, &self.last_insertion.1);
                    self.update_cursor_from_char_idx(idx + self.last_insertion.1.len() - 1);
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
                    should_update_preferred_y = true;
                } else {
                    return;
                }
            }
            (
                None,
                _,
                Some(command_parser::Modifier::Find {
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
            (Some(Motion::Undo), None, None) => {
                if let Some(a) = self.undo_vec.pop() {
                    let _len = a.undo(&mut self.rope);
                    if self.cursor_pos.preferred_x == usize::MAX {
                        cursor_target_idx = self
                            .rope
                            .line_to_char(self.cursor_pos.preferred_y + 1)
                            .saturating_sub(2);
                    } else {
                        cursor_target_idx = self.rope.line_to_char(self.cursor_pos.preferred_y)
                            + self.cursor_pos.preferred_x;
                    }
                    self.redo_vec.push(a);
                }
            }
            (Some(Motion::Redo), None, None) => {
                if let Some(a) = self.redo_vec.pop() {
                    let len = a.execute(&mut self.rope);
                    cursor_target_idx += len;
                    self.undo_vec.push(a);
                }
            }
            _ => {}
        }

        // update selection range
        match (&self.mode, command.modifier) {
            (Mode::VisualLine(y), None) => {
                // if cursor is after ancor, ancor is at start of line
                // else ancor is at end of line
                if let Some(sel) = self.selections.first_mut() {
                    if cursor_target_idx >= sel.ancor {
                        sel.ancor = self.rope.line_to_char(*y);
                        sel.cursor = line_end_idx(cursor_target_idx, &self.rope);
                    } else {
                        sel.ancor = line_end_idx(self.rope.line_to_char(*y), &self.rope);
                        let curr_line = self.rope.char_to_line(cursor_target_idx);
                        sel.cursor = self.rope.line_to_char(curr_line);
                    }
                }
            }
            (Mode::Visual, None) => {
                if let Some(sel) = self.selections.first_mut() {
                    sel.cursor = cursor_target_idx;
                }
            }
            (Mode::VisualBlock, _) => {
                self.selections.clear();
            }
            _ => {
                let new_sel = VisualSelection {
                    ancor: range.0,
                    cursor: range.1.saturating_sub(1),
                };
                self.selections.clear();
                self.selections.push(new_sel);
            }
        }

        // sync char range to visual selection
        if visual_mode {
            should_save_command = false;
            if let Some(sel) = self.selections.first_mut() {
                let start_select_rng = sel.ancor.min(sel.cursor);
                let mut end_select_rng = sel.ancor.max(sel.cursor);
                match command.action {
                    Some(Action::Delete) => end_select_rng += 1,
                    Some(Action::Change) => end_select_rng += 1,
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
                        for y in y_rng[0]..=y_rng[1] {
                            let line_char = self.rope.line_to_char(y);
                            let start = line_char + x_rng[0];
                            let end = line_char + x_rng[1];
                            if let Some(slice) = self.rope.get_slice(start..=end) {
                                slices.push(slice.to_string());
                            }
                        }
                        let buf = YankBuffer::Block(slices);
                        self.yank_buffer.insert('"', buf);
                    }
                } else if let Some(slice) = self.rope.get_slice(range.0..range.1) {
                    let mut yank_lines = false;
                    for c in slice.chars() {
                        if c == '\n' {
                            yank_lines = true;
                            break;
                        }
                    }
                    let new_content = if yank_lines {
                        YankBuffer::Lines(String::from(slice))
                    } else {
                        YankBuffer::Chars(String::from(slice))
                    };
                    self.yank_buffer.insert('"', new_content);
                }
            }
            _ => {}
        }

        // check for action
        match command.action {
            Some(Action::Yank) => {
                if !visual_mode {
                    self.highlight_yank = true;
                }
                self.cursor_pos.preferred_x = self.cursor_pos.x;
                self.cursor_pos.preferred_y = self.cursor_pos.y;
                self.change_mode(Mode::Normal);
                should_move_cursor = false;
            }
            Some(Action::Delete) | Some(Action::Change) => {
                let action = undo::Action::Delete {
                    idx: range.0,
                    content: self.rope.slice(range.0..range.1).into(),
                };
                self.undo_vec.push(action);
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
            self.last_insertion.1.clear();
            self.last_insertion.0 = self.get_char_idx();
            match command.motion {
                Some(Motion::NewLineBelow) | Some(Motion::NewLineAbove) => {
                    self.last_insertion.1.push_str("\n")
                }
                _ => {}
            }
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

        if should_update_preferred_y {
            self.cursor_pos.preferred_y = self.cursor_pos.y;
        }

        // check for matching bracket
        self.matching_bracket_idx = find_matching_bracket(self.get_char_idx(), &self.rope);

        self.scroll(self.cursor_pos.y);
    }

    fn insert_text(&mut self, e: KeyEvent) {
        let mut text_to_insert = None;
        let idx = self.get_char_idx();
        match e.code {
            KeyCode::Char(c) => {
                let mut text = String::from(c);
                if let Some(pair) = PAIRS.iter().find(|e| e.contains(&c)) {
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
                self.last_insertion.1.pop();

                if x > 0 {
                    // NORMAL BACKSPACE: Just delete the char to the left

                    // check for bracket pair
                    if CLOSING_BRACKETS.contains(&self.rope.char(idx))
                        && OPENING_BRACKETS.contains(&self.rope.char(idx - 1))
                    {
                        self.rope.remove(idx - 1..=idx);
                    } else {
                        self.rope.remove(idx - 1..idx);
                    }

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
                let left_char = self.rope.char(idx.saturating_sub(1));
                let c = self.rope.char(idx);

                if OPENING_BRACKETS.contains(&left_char) && CLOSING_BRACKETS.contains(&c) {
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
                    self.last_insertion.1 += "\n";
                    let cursor_target_idx = idx + whitespace.len() + 5;
                    self.update_cursor_from_char_idx(cursor_target_idx);
                } else if OPENING_BRACKETS.contains(&left_char) {
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
                    self.last_insertion.1 += "\n";
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
            self.last_insertion.1 += &text;
        }
        self.scroll(self.cursor_pos.y);
    }

    fn scroll(&mut self, target_y: usize) {
        let offset = SCROLL_OFFSET;
        let height = self.main_height - 1 - offset;

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
        self.scroll(self.cursor_pos.y);
    }

    fn exit(&mut self) {
        self.mode = Mode::Exit;
    }

    fn change_mode(&mut self, target_mode: Mode) {
        match target_mode {
            Mode::Normal => {
                if self.mode != Mode::Search && self.mode != Mode::Command {
                    self.command_bar.clear();
                }
                if self.command_bar.len() == 1 {
                    self.command_bar.clear();
                }
                if self.mode == Mode::Insert {
                    let action = undo::Action::Insert {
                        idx: self.last_insertion.0,
                        content: self.last_insertion.1.clone(),
                    };
                    self.undo_vec.push(action);
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
                self.selections.clear();
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
