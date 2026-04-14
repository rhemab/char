use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::trie::*;

// motions are not dependant on actions and should execute immediately
#[derive(Debug, Clone, Copy)]
pub enum Motion {
    Up,
    Down,
    Left,
    Right,
    HalfScreenDown,
    HalfScreenUp,
    NextEmptyLine,
    PrevEmptyLine,
    LineStart,
    LineEnd,
    FirstWord,
    FileStart,
    FileEnd,
    Word,
    End,
    Back,
    UpperWord,
    UpperEnd,
    UpperBack,
    NewLineBelow,
    NewLineAbove,
    InsertMode,
    VisualMode,
    UpperInsert,
    Append,
    UpperAppend,
    UpperChange,
    UpperDelete,
    EnterCommandMode,
    EnterSearchMode,
    DeleteLine,
    ChangeLine,
    DeleteChar,
    YankLine,
    UpperYank,
    Paste,
    UpperPaste,
    NextSearchResult,
    PrevSearchResult,
    Repeat,
    OpenParen,
    OpenCurlyBrace,
    OpenBracket,
    OpenAngleBracket,
    DoubleQuote,
    SingleQuote,
    Backtick,
    Percent,
    Star,
    Comma,
    Semicolon,
    Substitute,
}

#[derive(Debug, Clone, Copy)]
pub enum Action {
    Delete,
    Change,
    Yank,
}

#[derive(Debug, Clone, Copy)]
pub enum Modifier {
    Inside,
    Around,
    Find {
        c: char,
        forwards: bool,
        inclusive: bool,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum PendingCmd {
    Find { forwards: bool, inclusive: bool },
}

#[derive(Default, Debug, Clone)]
pub struct Command {
    pub motion: Option<Motion>,
    pub action: Option<Action>,
    pub modifier: Option<Modifier>,
    pub count: String,
}

pub struct Parser {
    trie: TrieNode,
    pub motion_buffer: Vec<KeyEvent>,
    pub command: Option<Command>,
    pub input_buffer: String,
    pending_cmd: Option<PendingCmd>,
    last_find_cmd: Option<Modifier>,
}

impl Default for Parser {
    fn default() -> Self {
        Parser {
            trie: generate_trie(),
            motion_buffer: vec![],
            command: None,
            input_buffer: String::new(),
            pending_cmd: None,
            last_find_cmd: None,
        }
    }
}

impl Parser {
    pub fn reset(&mut self) {
        self.input_buffer.clear();
        self.motion_buffer.clear();
        self.command = None;
        self.pending_cmd = None;
    }

    pub fn generate_command(&mut self, key_event: KeyEvent, visual_mode: bool) -> Option<Command> {
        if let Some(pending_cmd) = self.pending_cmd {
            match (pending_cmd, key_event.code, key_event.modifiers) {
                (
                    PendingCmd::Find {
                        forwards,
                        inclusive,
                    },
                    KeyCode::Char(c),
                    KeyModifiers::NONE | KeyModifiers::SHIFT,
                ) => {
                    let find_cmd = Modifier::Find {
                        c,
                        forwards,
                        inclusive,
                    };
                    self.last_find_cmd = Some(find_cmd);
                    if let Some(cmd) = &mut self.command {
                        cmd.modifier = Some(find_cmd);
                        return Some(cmd.clone());
                    }
                    return Some(Command {
                        modifier: Some(find_cmd),
                        ..Command::default()
                    });
                }
                _ => {
                    self.reset();
                    return None;
                }
            }
        }

        match key_event.code {
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
                // check for number
                if let Some(n) = c.to_digit(10) {
                    if let Some(cmd) = &mut self.command {
                        match (cmd.action, n) {
                            (Some(Action::Yank), 0) => {}
                            _ => {
                                cmd.count.push(c);
                                return None;
                            }
                        }
                    } else if n != 0 {
                        self.command = Some(Command {
                            count: String::from(c),
                            ..Default::default()
                        });
                        return None;
                    }
                }
            }
            _ => {}
        }

        // check for action
        match (key_event.code, key_event.modifiers) {
            (KeyCode::Char('d'), KeyModifiers::NONE) => {
                // if in visual mode, send action
                if visual_mode {
                    return Some(Command {
                        action: Some(Action::Delete),
                        ..Default::default()
                    });
                }
                if let Some(cmd) = &mut self.command {
                    // if 'dd' delete line
                    match cmd.action {
                        Some(Action::Delete) => {
                            cmd.motion = Some(Motion::DeleteLine);
                            return Some(cmd.clone());
                        }
                        _ => {}
                    }
                    cmd.action = Some(Action::Delete);
                } else {
                    self.command = Some(Command {
                        action: Some(Action::Delete),
                        ..Default::default()
                    })
                }
                return None;
            }
            (KeyCode::Char('c'), KeyModifiers::NONE) => {
                // if in visual mode, send action
                if visual_mode {
                    return Some(Command {
                        action: Some(Action::Change),
                        ..Default::default()
                    });
                }
                if let Some(cmd) = &mut self.command {
                    // if 'cc' change line
                    match cmd.action {
                        Some(Action::Change) => {
                            cmd.motion = Some(Motion::ChangeLine);
                            return Some(cmd.clone());
                        }
                        _ => {}
                    }
                    cmd.action = Some(Action::Change);
                } else {
                    self.command = Some(Command {
                        action: Some(Action::Change),
                        ..Default::default()
                    })
                }
                return None;
            }
            (KeyCode::Char('y'), KeyModifiers::NONE) => {
                // if in visual mode, send action
                if visual_mode {
                    return Some(Command {
                        action: Some(Action::Yank),
                        ..Default::default()
                    });
                }
                if let Some(cmd) = &mut self.command {
                    // if 'yy' change line
                    match cmd.action {
                        Some(Action::Yank) => {
                            cmd.motion = Some(Motion::YankLine);
                            return Some(cmd.clone());
                        }
                        _ => {}
                    }
                    cmd.action = Some(Action::Yank);
                } else {
                    self.command = Some(Command {
                        action: Some(Action::Yank),
                        ..Default::default()
                    })
                }
                return None;
            }
            (KeyCode::Char('i'), KeyModifiers::NONE) => {
                if let Some(cmd) = &mut self.command {
                    if cmd.action.is_some() {
                        cmd.modifier = Some(Modifier::Inside);
                        return None;
                    }
                }
            }
            (KeyCode::Char('a'), KeyModifiers::NONE) => {
                if let Some(cmd) = &mut self.command {
                    if cmd.action.is_some() {
                        cmd.modifier = Some(Modifier::Around);
                        return None;
                    }
                }
            }
            (KeyCode::Char('t'), KeyModifiers::NONE) => {
                self.pending_cmd = Some(PendingCmd::Find {
                    forwards: true,
                    inclusive: false,
                });
                return None;
            }
            (KeyCode::Char('f'), KeyModifiers::NONE) => {
                self.pending_cmd = Some(PendingCmd::Find {
                    forwards: true,
                    inclusive: true,
                });
                return None;
            }
            (KeyCode::Char('T'), KeyModifiers::SHIFT) => {
                self.pending_cmd = Some(PendingCmd::Find {
                    forwards: false,
                    inclusive: false,
                });
                return None;
            }
            (KeyCode::Char('F'), KeyModifiers::SHIFT) => {
                self.pending_cmd = Some(PendingCmd::Find {
                    forwards: false,
                    inclusive: true,
                });
                return None;
            }
            _ => {}
        }

        // search for motion
        self.motion_buffer.push(key_event);
        if let Some(node) = self.trie.search(&self.motion_buffer) {
            if let Some(motion) = node.command {
                let mut count = String::from("1");
                if let Some(cmd) = &mut self.command {
                    match (cmd.action, cmd.modifier, motion) {
                        (Some(Action::Change), _, Motion::Word) => {
                            cmd.motion = Some(Motion::End);
                            return Some(cmd.clone());
                        }
                        (Some(Action::Change), _, Motion::UpperWord) => {
                            cmd.motion = Some(Motion::UpperEnd);
                            return Some(cmd.clone());
                        }
                        (Some(Action::Delete), _, Motion::Down) => {
                            cmd.motion = Some(Motion::DeleteLine);
                            cmd.count.clear();
                            cmd.count.push('2');
                            return Some(cmd.clone());
                        }
                        (Some(Action::Change), _, Motion::Down) => {
                            cmd.motion = Some(Motion::ChangeLine);
                            cmd.count.clear();
                            cmd.count.push('2');
                            return Some(cmd.clone());
                        }
                        (Some(Action::Yank), _, Motion::Down) => {
                            cmd.motion = Some(Motion::YankLine);
                            cmd.count.clear();
                            cmd.count.push('2');
                            return Some(cmd.clone());
                        }
                        (Some(_action), Some(_modifier), Motion::Back) => {
                            cmd.motion = Some(Motion::OpenParen);
                            return Some(cmd.clone());
                        }
                        (Some(_action), Some(_modifier), Motion::PrevEmptyLine) => {
                            cmd.motion = Some(Motion::OpenCurlyBrace);
                            return Some(cmd.clone());
                        }
                        (_, None, Motion::Comma) => match self.last_find_cmd {
                            Some(Modifier::Find {
                                c,
                                forwards,
                                inclusive,
                            }) => {
                                cmd.modifier = Some(Modifier::Find {
                                    c,
                                    forwards: !forwards,
                                    inclusive,
                                });
                                return Some(cmd.clone());
                            }
                            _ => {
                                self.reset();
                                return None;
                            }
                        },
                        (_, None, Motion::Semicolon) => match self.last_find_cmd {
                            Some(Modifier::Find {
                                c,
                                forwards,
                                inclusive,
                            }) => {
                                cmd.modifier = Some(Modifier::Find {
                                    c,
                                    forwards,
                                    inclusive,
                                });
                                return Some(cmd.clone());
                            }
                            _ => {
                                self.reset();
                                return None;
                            }
                        },
                        (None, _, _) => {
                            // if no action, extract count
                            count = cmd.count.clone();
                        }
                        _ => {
                            cmd.motion = Some(motion);
                            return Some(cmd.clone());
                        }
                    }
                }
                let mut cmd = Command::default();
                cmd.count = count;
                match motion {
                    Motion::UpperChange => {
                        cmd.motion = Some(motion);
                        cmd.action = Some(Action::Change);
                    }
                    Motion::UpperDelete => {
                        cmd.motion = Some(Motion::LineEnd);
                        cmd.action = Some(Action::Delete);
                    }
                    Motion::UpperYank => {
                        cmd.motion = Some(Motion::LineEnd);
                        cmd.action = Some(Action::Yank);
                    }
                    Motion::DeleteChar => {
                        cmd.motion = Some(Motion::Right);
                        cmd.action = Some(Action::Delete);
                    }
                    Motion::Comma => match self.last_find_cmd {
                        Some(Modifier::Find {
                            c,
                            forwards,
                            inclusive,
                        }) => {
                            cmd.modifier = Some(Modifier::Find {
                                c,
                                forwards: !forwards,
                                inclusive,
                            });
                        }
                        _ => {
                            self.reset();
                            return None;
                        }
                    },
                    Motion::Semicolon => match self.last_find_cmd {
                        Some(Modifier::Find {
                            c,
                            forwards,
                            inclusive,
                        }) => {
                            cmd.modifier = Some(Modifier::Find {
                                c,
                                forwards,
                                inclusive,
                            });
                        }
                        _ => {
                            self.reset();
                            return None;
                        }
                    },
                    Motion::ChangeLine | Motion::Substitute => {
                        cmd.action = Some(Action::Change);
                        cmd.motion = Some(motion);
                    }
                    _ => {
                        cmd.motion = Some(motion);
                    }
                }
                return Some(cmd);
            }
            // return none here so that the buffer doesn't reset
            // because we found a node but not yet a command
            return None;
        }

        // reset here because we didn't find a node
        self.reset();
        None
    }
}

fn generate_trie() -> TrieNode {
    let mut trie = TrieNode::default();

    trie.insert(
        &[KeyEvent::new(KeyCode::Char('j'), KeyModifiers::empty())],
        Motion::Down,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('k'), KeyModifiers::empty())],
        Motion::Up,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('h'), KeyModifiers::empty())],
        Motion::Left,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('l'), KeyModifiers::empty())],
        Motion::Right,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char(':'), KeyModifiers::empty())],
        Motion::EnterCommandMode,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('/'), KeyModifiers::empty())],
        Motion::EnterSearchMode,
    );
    trie.insert(
        &[
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::empty()),
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::empty()),
        ],
        Motion::FileStart,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('0'), KeyModifiers::empty())],
        Motion::LineStart,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('$'), KeyModifiers::empty())],
        Motion::LineEnd,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('^'), KeyModifiers::empty())],
        Motion::FirstWord,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('_'), KeyModifiers::empty())],
        Motion::FirstWord,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('G'), KeyModifiers::empty())],
        Motion::FileEnd,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('w'), KeyModifiers::empty())],
        Motion::Word,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('W'), KeyModifiers::empty())],
        Motion::UpperWord,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('e'), KeyModifiers::empty())],
        Motion::End,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('E'), KeyModifiers::empty())],
        Motion::UpperEnd,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('b'), KeyModifiers::empty())],
        Motion::Back,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('B'), KeyModifiers::empty())],
        Motion::UpperBack,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('o'), KeyModifiers::empty())],
        Motion::NewLineBelow,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('O'), KeyModifiers::empty())],
        Motion::NewLineAbove,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('i'), KeyModifiers::empty())],
        Motion::InsertMode,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('I'), KeyModifiers::empty())],
        Motion::UpperInsert,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('a'), KeyModifiers::empty())],
        Motion::Append,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('A'), KeyModifiers::empty())],
        Motion::UpperAppend,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('v'), KeyModifiers::empty())],
        Motion::VisualMode,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)],
        Motion::HalfScreenDown,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)],
        Motion::HalfScreenUp,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('}'), KeyModifiers::empty())],
        Motion::NextEmptyLine,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('{'), KeyModifiers::empty())],
        Motion::PrevEmptyLine,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('p'), KeyModifiers::empty())],
        Motion::Paste,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('P'), KeyModifiers::empty())],
        Motion::UpperPaste,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('n'), KeyModifiers::empty())],
        Motion::NextSearchResult,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('N'), KeyModifiers::empty())],
        Motion::PrevSearchResult,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('.'), KeyModifiers::empty())],
        Motion::Repeat,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('C'), KeyModifiers::empty())],
        Motion::UpperChange,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('D'), KeyModifiers::empty())],
        Motion::UpperDelete,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::empty())],
        Motion::UpperYank,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty())],
        Motion::DeleteChar,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('('), KeyModifiers::empty())],
        Motion::OpenParen,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('['), KeyModifiers::empty())],
        Motion::OpenBracket,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('<'), KeyModifiers::empty())],
        Motion::OpenAngleBracket,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('\''), KeyModifiers::empty())],
        Motion::SingleQuote,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('"'), KeyModifiers::empty())],
        Motion::DoubleQuote,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('`'), KeyModifiers::empty())],
        Motion::Backtick,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('%'), KeyModifiers::empty())],
        Motion::Percent,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('*'), KeyModifiers::empty())],
        Motion::Star,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char(','), KeyModifiers::empty())],
        Motion::Comma,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char(';'), KeyModifiers::empty())],
        Motion::Semicolon,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('s'), KeyModifiers::empty())],
        Motion::Substitute,
    );
    trie.insert(
        &[KeyEvent::new(KeyCode::Char('S'), KeyModifiers::empty())],
        Motion::ChangeLine,
    );

    trie
}
