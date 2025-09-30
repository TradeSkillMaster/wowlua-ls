use std::collections::HashMap;

use rowan::GreenNode;
use crate::ast::*;
use crate::syntax::SyntaxNode;

enum ValueType {
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
    file: String,
    block_index: Vec<usize>,
    offset_from_block: usize,
    value_type: ValueType,
}

fn get_expression_type(expression: Expression) {

}

pub fn get_types(green: GreenNode, filename: &str) {
    let root = SyntaxNode::new_root(green);
    let mut block_queue: Vec<SyntaxNode> = Vec::new();
    block_queue.push(root.clone());
    let mut indexes: Vec<usize> = Vec::new();
    let mut block_indexes: Vec<usize> = Vec::new();
    block_indexes.push(0);
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
                    let offset_from_block = usize::from(a.syntax().text_range().start() - block.syntax().text_range().start());
                    let block_index = block_indexes.clone();
                    let file = String::from(filename);
                    //let id = Identifier{offset_from_block, file, block_index};
                }
                _ => {}
            }
        }
    }
}
