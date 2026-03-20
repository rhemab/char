use crossterm::event::KeyEvent;

use crate::commands::*;

pub struct TrieNode {
    pub command: Option<Motion>,
    children: Vec<(KeyEvent, TrieNode)>,
}

impl Default for TrieNode {
    fn default() -> Self {
        TrieNode {
            command: None,
            children: vec![],
        }
    }
}

impl TrieNode {
    /// insert a series of key events that lead to a command
    pub fn insert(&mut self, input_events: &[KeyEvent], command: Motion) {
        let mut current_node = self;
        for e in input_events {
            // check if current_node exists
            // if not, create new node
            if let Some(i) = current_node
                .children
                .iter_mut()
                .position(|(event, _)| e == event)
            {
                // if exists, move to that node
                current_node = &mut current_node.children[i].1;
            } else {
                // if doesn't exist, create it and move to it
                current_node.children.push((*e, TrieNode::default()));
                let i = current_node.children.len().saturating_sub(1);
                current_node = &mut current_node.children[i].1;
            }
        }

        // add the command to the last node
        current_node.command = Some(command);
    }

    /// search for a key within the current node
    /// if a found, return a reference to that node
    /// if not found, return none
    pub fn search<'a>(
        &self,
        input_event: &KeyEvent,
        current_node: &'a TrieNode,
    ) -> Option<&'a TrieNode> {
        if let Some(i) = current_node
            .children
            .iter()
            .position(|(e, _)| input_event == e)
        {
            return Some(&current_node.children[i].1);
        }

        None
    }
}
