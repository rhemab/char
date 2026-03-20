use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::trie::*;

// motions are not dependant on actions
#[derive(Debug, Clone, Copy)]
pub enum Motion {
    Up,
    Down,
    Left,
    Right,
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
    Insert,
    Append,
    // Paste,
    EnterCommandMode,
    DeleteLine,
    // ChangeLine,
}

#[derive(Default, Debug, Clone)]
pub struct Command {
    pub motion: Option<Motion>,
    pub count: String,
}

pub struct Parser {
    trie: TrieNode,
    input_buffer: Vec<KeyEvent>,
    pub command: Option<Command>,
    pub cmd_buffer: String,
}

impl Default for Parser {
    fn default() -> Self {
        Parser {
            trie: generate_trie(),
            input_buffer: vec![],
            command: None,
            cmd_buffer: String::new(),
        }
    }
}

impl Parser {
    pub fn generate_command(&mut self, key_event: KeyEvent) -> Option<Command> {
        self.input_buffer.push(key_event);
        let mut current_node = &self.trie;
        for e in &self.input_buffer {
            if let Some(node) = self.trie.search(&e, current_node) {
                current_node = node;
                if let Some(motion) = node.command {
                    self.input_buffer.clear();
                    return Some(Command {
                        motion: Some(motion),
                        ..Default::default()
                    });
                }
                match e.code {
                    KeyCode::Char(c) => {
                        self.cmd_buffer.push(c);
                    }
                    _ => {}
                }
            } else {
                self.input_buffer.clear();
                return None;
            }
        }

        None
    }
}

fn char_to_key_event(c: char) -> KeyEvent {
    let code = KeyCode::Char(c);
    let modifiers = KeyModifiers::empty();
    KeyEvent::new(code, modifiers)
}

fn generate_trie() -> TrieNode {
    let mut trie = TrieNode::default();

    trie.insert(&[char_to_key_event('j')], Motion::Down);
    trie.insert(&[char_to_key_event('k')], Motion::Up);
    trie.insert(&[char_to_key_event('h')], Motion::Left);
    trie.insert(&[char_to_key_event('l')], Motion::Right);
    trie.insert(&[char_to_key_event(':')], Motion::EnterCommandMode);
    trie.insert(&[char_to_key_event(':')], Motion::EnterCommandMode);
    trie.insert(
        &[char_to_key_event('g'), char_to_key_event('g')],
        Motion::FileStart,
    );
    trie.insert(
        &[char_to_key_event('d'), char_to_key_event('d')],
        Motion::DeleteLine,
    );
    trie.insert(&[char_to_key_event('0')], Motion::LineStart);
    trie.insert(&[char_to_key_event('$')], Motion::LineEnd);
    trie.insert(&[char_to_key_event('^')], Motion::FirstWord);
    trie.insert(&[char_to_key_event('G')], Motion::FileEnd);
    trie.insert(&[char_to_key_event('w')], Motion::Word);
    trie.insert(&[char_to_key_event('W')], Motion::UpperWord);
    trie.insert(&[char_to_key_event('e')], Motion::End);
    trie.insert(&[char_to_key_event('E')], Motion::UpperEnd);
    trie.insert(&[char_to_key_event('b')], Motion::Back);
    trie.insert(&[char_to_key_event('B')], Motion::UpperBack);
    trie.insert(&[char_to_key_event('o')], Motion::NewLineBelow);
    trie.insert(&[char_to_key_event('O')], Motion::NewLineAbove);
    trie.insert(&[char_to_key_event('i')], Motion::Insert);
    trie.insert(&[char_to_key_event('a')], Motion::Append);

    trie
}
