use std::collections::HashMap;

use rowan::GreenNode;
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

struct TypeInstance {
    tree: Vec<(String, bool)>
}

fn node_to_position(node: &SyntaxNode) -> u32 {
    u32::from(node.text_range().start())
}

fn get_types(green: GreenNode, filename: &str) -> Option<()> {
    let root = SyntaxNode::new_root(green);
    let mut current_node = root;

    let mut values: HashMap<String, Type> = HashMap::new();
    let mut scoped: Vec<(String, Type)> = Vec::new();
    let mut scope_starts: Vec<usize> = Vec::new();
    let mut global: HashMap<String, Type> = HashMap::new();

    enum Switch {
        Sibling,
        Parent,
        Child,
        None
    }

    let mut switch = Switch::None;
    loop {
        match switch {
            Switch::Sibling => {
                switch = Switch::None;
                match current_node.next_sibling() {
                    Some(n) => current_node = n,
                    None => switch = Switch::Parent,
                }
            },
            Switch::Parent => {
                switch = Switch::Sibling;
                match current_node.parent() {
                    Some(n) => current_node = n,
                    None => break,
                };
            }
            Switch::Child => {
                switch = Switch::None;
                match current_node.first_child() {
                    Some(n) => current_node = n,
                    None => switch = Switch::Sibling,
                }
            }
            _ => (),
        }
        match current_node.kind() {
            SyntaxKind::Block => {
                switch = Switch::Child;
                scope_starts.push(scoped.len());
            },
            SyntaxKind::LocalAssignStatement => {
                let variable_list = current_node.first_child_by_kind(&|k| k == SyntaxKind::VariableList)?;
                let expression_list = current_node.first_child_by_kind(&|k| k == SyntaxKind::VariableList);

                let expression_or_name = |k| k == SyntaxKind::Expression || k == SyntaxKind::GroupedExpression || k == SyntaxKind::Name;

                let var = variable_list.first_child_by_kind(&expression_or_name);
                let exp = match &expression_list {
                    Some(el) => el.first_child_by_kind(&|k| k == SyntaxKind::Expression),
                    None => None,
                };
                loop {
                    match &var {
                        None => break,
                        Some(v) => {
                            scoped.push((v.text().to_string(), Type::Missing));
                            values.insert(get_identifier(filename, node_to_position(v)), Type::Missing);
                        }
                    }
                }
            }
            _ => (),
        }
    }

    return Some(())
}
