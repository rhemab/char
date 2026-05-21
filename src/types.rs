use ropey::Rope;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

use crate::command_parser::*;
use crate::undo;

use std::collections::HashMap;

#[derive(Default)]
pub struct Highlight {
    pub syntax_set: SyntaxSet,
    pub theme_set: ThemeSet,
}

#[derive(Default)]
pub struct App {
    pub highlight: Highlight,
    pub show_first_time_popup: bool,
    pub lines_in_view: [usize; 2],
    pub last_command: Command,
    pub last_insertion: (usize, String),
    pub redraw: bool,
    pub dirty: bool,
    pub mode: Mode,
    pub parser: Parser,
    pub cursor_pos: CursorPos,
    pub top_line: usize,
    pub main_height: usize,
    pub rope: Rope,
    pub command_bar: String,
    pub path: String,
    pub selections: Vec<VisualSelection>,
    pub yank_buffer: HashMap<char, YankBuffer>,
    pub highlight_yank: bool,
    pub query: String,
    pub visual_block_rng: Option<VisualBlockRng>,
    pub matching_bracket_idx: Option<usize>,
    pub undo_vec: Vec<undo::Action>,
    pub redo_vec: Vec<undo::Action>,
}

#[derive(Clone)]
pub enum YankBuffer {
    Chars(String),
    Lines(String),
    Block(Vec<String>),
}

#[derive(Default, Debug)]
pub struct VisualBlockRng {
    pub x_rng: [usize; 2],
    pub y_rng: [usize; 2],
}

#[derive(Default, Debug, PartialEq)]
pub struct VisualSelection {
    pub ancor: usize,
    pub cursor: usize,
}

#[derive(Default, Debug)]
pub struct CursorPos {
    pub x: usize,
    pub y: usize,
    pub preferred_x: usize,
    pub preferred_y: usize,
}

#[derive(Debug, PartialEq)]
pub enum Mode {
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
