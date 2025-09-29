use std::collections::HashMap;

use rowan::GreenNode;
use crate::ast::*;
use crate::syntax::{SyntaxNode, SyntaxKind};

enum Type {
    Nil,
    Boolean,
    Number,
    String,
    Function,
    Thread,
    Table,
    Missing,
    Relay(String),
}

struct Identifier {

}

fn get_identifier(filename: &str, absolute_position: u32) -> String {
    format!("{}-{}", filename, absolute_position)
}

pub struct TypeInstance {
    tree: Vec<(String, bool)>
}

fn node_to_position(node: &SyntaxNode) -> u32 {
    u32::from(node.text_range().start())
}

pub fn get_types(green: GreenNode, filename: &str) -> Vec<TypeInstance> {
    let root = SyntaxNode::new_root(green);
    let mut block_queue: Vec<SyntaxNode> = Vec::new();
    block_queue.push(root.clone());
    let mut indexes: Vec<usize> = Vec::new();
    indexes.push(0);

    let mut block = Block::cast(root).expect("everything starts with a block");

    loop {
        let mut all_statements = block.statements();
        if indexes[indexes.len() - 1] >= all_statements.len() {
            indexes.pop();
            block_queue.pop();
            if block_queue.len() > 0 {
                block = Block::cast(block_queue[block_queue.len() - 1].clone()).expect("block expected due to past queue");
                all_statements = block.statements();
            } else {
                break;
            }
        }
        for i in (indexes[indexes.len() - 1])..all_statements.len() { 
            let len = indexes.len();
            indexes[len - 1] = i + 1;
            let statement = &all_statements[i];
            match statement {
                Statement::LocalAssign(a) => {

                }
                _ => {}
            }
        }
    }

    return Vec::new()
}
