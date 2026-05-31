use crate::analysis::AnalysisResult;
use crate::syntax::{NodeOrToken, SyntaxKind, SyntaxNode, SyntaxToken};
use crate::syntax::tree::SyntaxTree;
use super::{DiagnosticPass, WowDiagnostic};

const FUNCTION_LEVEL_TAGS: &[&str] = &[
    "param", "return", "overload", "generic", "nodiscard", "deprecated",
    "constructor", "builds-field", "built-name", "built-extends",
    "type-narrows", "defclass", "narrows-arg", "requires",
];

const CLASS_VALID_TAGS: &[&str] = &["overload", "deprecated", "constructor"];
const EVENT_VALID_TAGS: &[&str] = &["param"];

pub(crate) struct DocFuncNoFunction;

impl DiagnosticPass for DocFuncNoFunction {
    fn run(&self, _analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let root = SyntaxNode::new_root(tree);

        let mut func_tags: Vec<(u32, u32, &str)> = Vec::new();
        let mut has_class = false;
        let mut has_event = false;
        let mut prev_was_newline = false;

        for event in root.descendants_with_tokens() {
            let NodeOrToken::Token(tok) = event else { continue };
            let kind = tok.kind();
            if kind == SyntaxKind::Comment {
                let text = tok.text();
                if let Some(after_at) = text.strip_prefix("---@")
                    .or_else(|| text.strip_prefix("---").and_then(|s| s.trim_start().strip_prefix('@')))
                {
                    let tag = after_at.split(|c: char| c.is_whitespace()).next().unwrap_or("");
                    if tag == "class" {
                        has_class = true;
                    } else if tag == "event" {
                        has_event = true;
                    } else if FUNCTION_LEVEL_TAGS.contains(&tag) {
                        let r = tok.text_range();
                        func_tags.push((u32::from(r.start()), u32::from(r.end()), tag));
                    }
                }
                prev_was_newline = false;
            } else if kind == SyntaxKind::Newline {
                if prev_was_newline && !func_tags.is_empty() {
                    flush(&func_tags, has_class, has_event, diags);
                    func_tags.clear();
                    has_class = false;
                    has_event = false;
                }
                prev_was_newline = true;
            } else if kind == SyntaxKind::Whitespace {
                // don't change state
            } else {
                if !func_tags.is_empty() {
                    if !token_precedes_function(&tok) {
                        flush(&func_tags, has_class, has_event, diags);
                    }
                    func_tags.clear();
                    has_class = false;
                    has_event = false;
                }
                prev_was_newline = false;
            }
        }
        if !func_tags.is_empty() {
            flush(&func_tags, has_class, has_event, diags);
        }
    }
}

fn token_precedes_function(tok: &SyntaxToken<'_>) -> bool {
    let tok_start = u32::from(tok.text_range().start());
    let mut node = tok.parent();
    while let Some(n) = node {
        let n_start = u32::from(n.text_range().start());
        if n_start != tok_start {
            break;
        }
        match n.kind() {
            SyntaxKind::FunctionDefinition => return true,
            SyntaxKind::LocalAssignStatement | SyntaxKind::AssignStatement => {
                return statement_has_function_value(&n);
            }
            SyntaxKind::Field => {
                return field_has_function_value(&n);
            }
            SyntaxKind::FunctionCall | SyntaxKind::MethodCall => {
                return call_has_function_argument(&n);
            }
            _ => {}
        }
        node = n.parent();
    }
    false
}

fn field_has_function_value(field: &SyntaxNode<'_>) -> bool {
    for child in field.children() {
        if child.kind() == SyntaxKind::FunctionDefinition {
            return true;
        }
        if has_function_in_binary_expr(&child) {
            return true;
        }
    }
    false
}

fn statement_has_function_value(stmt: &SyntaxNode<'_>) -> bool {
    for child in stmt.children() {
        if child.kind() == SyntaxKind::ExpressionList {
            for expr_child in child.children() {
                if expr_child.kind() == SyntaxKind::FunctionDefinition {
                    return true;
                }
                if has_function_in_binary_expr(&expr_child) {
                    return true;
                }
            }
        }
    }
    false
}

fn has_function_in_binary_expr(node: &SyntaxNode<'_>) -> bool {
    if node.kind() != SyntaxKind::BinaryExpression {
        return false;
    }
    for child in node.children() {
        if child.kind() == SyntaxKind::FunctionDefinition {
            return true;
        }
        if has_function_in_binary_expr(&child) {
            return true;
        }
    }
    false
}

fn call_has_function_argument(call: &SyntaxNode<'_>) -> bool {
    for child in call.children() {
        if child.kind() == SyntaxKind::ArgumentList {
            for arg in child.children() {
                if arg.kind() == SyntaxKind::FunctionDefinition {
                    return true;
                }
                if has_function_in_binary_expr(&arg) {
                    return true;
                }
            }
        }
    }
    false
}

fn flush(func_tags: &[(u32, u32, &str)], has_class: bool, has_event: bool, diags: &mut Vec<WowDiagnostic>) {
    for &(start, end, tag) in func_tags {
        if has_class && CLASS_VALID_TAGS.contains(&tag) {
            continue;
        }
        if has_event && EVENT_VALID_TAGS.contains(&tag) {
            continue;
        }
        super::DOC_FUNC_NO_FUNCTION.emit(
            diags,
            format!("@{} is not attached to a function definition", tag),
            start as usize,
            end as usize,
        );
    }
}
