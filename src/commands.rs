use std::collections::HashMap;

use crossterm::event::KeyCode;

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
    // Paste,
}

// actions are dependant on motions
#[derive(Debug, Clone, Copy)]
pub enum Action {
    Delete,
    Change,
}

// globals commands are independant
#[derive(Debug, Clone, Copy)]
pub enum Global {
    FileStart,
}

// globals leader keys
#[derive(Debug, Clone, Copy)]
pub enum LeaderKey {
    G,
    Collon,
}

#[derive(Default, Debug, Clone)]
pub struct Command {
    pub motion: Option<Motion>,
    pub action: Option<Action>,
    pub global: Option<Global>,
    pub count: String,
}

#[derive(Debug)]
pub struct Parser {
    pub command: Option<Command>,
    pub motion_map: HashMap<char, Motion>,
    pub action_map: HashMap<char, Action>,
    pub leader_keys: HashMap<char, LeaderKey>,
    pub global_cmd_map: HashMap<String, Global>,
    pub cmd_buffer: String,
}

impl Default for Parser {
    fn default() -> Self {
        Parser {
            command: None,
            motion_map: generate_motion_map(),
            action_map: generate_action_map(),
            leader_keys: generate_leader_key_map(),
            global_cmd_map: generate_global_cmd_map(),
            cmd_buffer: String::new(),
        }
    }
}

impl Parser {
    pub fn generate_command(&mut self, key_code: KeyCode) -> Option<Command> {
        match key_code {
            KeyCode::Char(c) => {
                eprintln!("char: {:?}", c);
                // check for number
                if c.is_ascii_digit() {
                    if let Some(command) = &mut self.command {
                        command.count.push(c);
                        self.cmd_buffer.push(c);
                        return None;
                    } else if !char_is_zero(c) {
                        let command = Command {
                            count: String::from(c),
                            ..Default::default()
                        };
                        self.command = Some(command);
                        self.cmd_buffer.push(c);
                    }
                }
                // check for motion
                if let Some(motion) = self.motion_map.get(&c) {
                    self.cmd_buffer.push(c);
                    if let Some(command) = &mut self.command {
                        command.motion = Some(motion.clone());
                    } else {
                        let command = Command {
                            motion: Some(motion.clone()),
                            count: 1.to_string(),
                            ..Default::default()
                        };
                        self.command = Some(command);
                    }
                    return self.command.clone();
                }

                // check for action
                if let Some(_action) = self.action_map.get(&c) {}

                // check for leader keys
                // then push to a buffer
                // then check buffer against hashmap
                if let Some(_leader) = self.leader_keys.get(&c) {
                    self.cmd_buffer.push(c);
                    if let Some(cmd) = self.global_cmd_map.get(&self.cmd_buffer) {
                        let command = Command {
                            global: Some(*cmd),
                            ..Default::default()
                        };
                        return Some(command);
                    }
                }
            }
            _ => {}
        }

        None
    }
}

fn char_is_zero(c: char) -> bool {
    if let Some(n) = c.to_digit(10) {
        if n == 0 {
            return true;
        }
    }
    false
}

fn generate_motion_map() -> HashMap<char, Motion> {
    let mut map = HashMap::new();

    map.insert('j', Motion::Down);
    map.insert('k', Motion::Up);
    map.insert('h', Motion::Left);
    map.insert('l', Motion::Right);

    map.insert('0', Motion::LineStart);
    map.insert('$', Motion::LineEnd);
    map.insert('^', Motion::FirstWord);
    map.insert('G', Motion::FileEnd);

    map.insert('w', Motion::Word);
    map.insert('W', Motion::UpperWord);
    map.insert('e', Motion::End);
    map.insert('E', Motion::UpperEnd);
    map.insert('b', Motion::Back);
    map.insert('B', Motion::UpperBack);

    map.insert('o', Motion::NewLineBelow);
    map.insert('O', Motion::NewLineAbove);

    map.insert('i', Motion::InsertMode);

    map
}

fn generate_action_map() -> HashMap<char, Action> {
    let mut map = HashMap::new();

    map.insert('d', Action::Delete);
    map.insert('c', Action::Change);

    map
}

fn generate_leader_key_map() -> HashMap<char, LeaderKey> {
    let mut map = HashMap::new();

    map.insert('g', LeaderKey::G);
    map.insert(':', LeaderKey::Collon);

    map
}

fn generate_global_cmd_map() -> HashMap<String, Global> {
    let mut map = HashMap::new();

    map.insert("gg".to_string(), Global::FileStart);

    map
}
