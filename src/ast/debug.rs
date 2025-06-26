//Copyright (C) 2025-  plusmouse and other contributors
//
//This program is free software: you can redistribute it and/or modify
//it under the terms of the GNU General Public License as published by
//the Free Software Foundation, either version 3 of the License, or
//(at your option) any later version.
//
//This program is distributed in the hope that it will be useful,
//but WITHOUT ANY WARRANTY; without even the implied warranty of
//MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//GNU General Public License for more details.
//
//You should have received a copy of the GNU General Public License
//along with this program.  If not, see <https://www.gnu.org/licenses/>.

use crate::ast::syntax;

fn dump_nodes(node: &syntax::SyntaxNode, indent: i32) {
    let mut counter = indent * 2;
    while counter > 0 {
        print!(" ");
        counter = counter - 1;
    }
    print!("Node: {:?}, {:?}\n", node.kind(), node.text_range());
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => {
                dump_nodes(&n, indent + 1);
            },
            rowan::NodeOrToken::Token(t) => {
                let mut counter = (indent + 1) * 2;
                while counter > 0 {
                    print!(" ");
                    counter = counter - 1;
                }
                let mut text = "";
                if t.text() != "\n" {
                    text = t.text()
                }
                print!("{:?}, {:?}, \"{}\"\n", t.kind(), t.text_range(), text);
            }
        }
    }
}
#[allow(dead_code)]
pub fn print_tree(green: &rowan::GreenNode) {
    let root = syntax::SyntaxNode::new_root(green.clone());
    dump_nodes(&root, 0);
}
