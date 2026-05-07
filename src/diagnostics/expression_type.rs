use crate::analysis::AnalysisResult;
use crate::syntax::parser::Parser;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::{SyntaxKind, SyntaxNode};
use crate::types::{TableIndex, ValueType};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct ExpressionType;

impl DiagnosticPass for ExpressionType {
    fn run(&self, analysis: &AnalysisResult, _tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for (&expr_id, arg_info) in &analysis.ir.expression_args {
            let table_idxs = &arg_info.table_idxs;
            let expected_return = &arg_info.return_type;
            let (str_start, str_end) = arg_info.str_range;
            let Some(raw_content) = analysis.ir.string_literals.get(&expr_id) else { continue };
            // string_literals stores trimmed content for "..." and '...' strings,
            // but for [[...]] long bracket strings, the brackets are still present.
            let content = strip_long_brackets(raw_content);
            let content_start = compute_content_start(content.len(), str_start, str_end);

            // Parse the expression as "return <expr>"
            let wrapped = format!("return {}", content);
            let expr_tree = Parser::new(&wrapped).parse();
            let prefix_len = 7u32; // "return ".len()

            // Walk the parsed expression tree and check identifiers
            let root = SyntaxNode::new_root(&expr_tree);
            for token in root.descendants_with_tokens().filter_map(|it| it.into_token()) {
                if token.kind() != SyntaxKind::Name {
                    continue;
                }
                let word = token.text();
                // Check if this identifier is a field of any context class
                if !table_idxs.iter().any(|&idx| analysis.get_field(idx, word).is_some()) {
                    let inner_start = u32::from(token.text_range().start());
                    let inner_end = u32::from(token.text_range().end());
                    let file_start = content_start + inner_start - prefix_len;
                    let file_end = content_start + inner_end - prefix_len;
                    let class_name = format_class_names(analysis, table_idxs);
                    super::UNDEFINED_FIELD.emit(
                        diags,
                        format!("undefined field '{}' in expression (not a field of '{}')", word, class_name),
                        file_start as usize,
                        file_end as usize,
                    );
                }
            }

            // Check return type constraint if specified
            if let Some(expected) = expected_return {
                let inferred = infer_expression_type(analysis, &expr_tree, table_idxs);
                if let Some(ref inferred_type) = inferred
                    && !is_assignable(inferred_type, expected)
                {
                    let inferred_str = analysis.format_type_depth(inferred_type, 0);
                    let expected_str = analysis.format_type_depth(expected, 0);
                    super::TYPE_MISMATCH.emit(
                        diags,
                        format!(
                            "expression returns '{}', expected '{}'",
                            inferred_str, expected_str
                        ),
                        str_start as usize,
                        str_end as usize,
                    );
                }
            }
        }
    }
}

/// Format class names from multiple table indices for error messages.
fn format_class_names(analysis: &AnalysisResult, table_idxs: &[TableIndex]) -> String {
    let names: Vec<&str> = table_idxs.iter()
        .map(|&idx| analysis.table(idx).class_name.as_deref().unwrap_or("?"))
        .collect();
    names.join(" & ")
}

/// Strip long bracket delimiters from string content.
/// `string_literals` stores `[[...]]` and `[=[...]=]` with their brackets intact.
pub fn strip_long_brackets(s: &str) -> &str {
    if let Some(rest) = s.strip_prefix("[[") {
        rest.strip_suffix("]]").unwrap_or(rest)
    } else if s.starts_with("[=") {
        let eq_count = s[1..].bytes().take_while(|&b| b == b'=').count();
        let open_len = 2 + eq_count;
        &s[open_len..s.len().saturating_sub(open_len)]
    } else {
        s
    }
}

/// Compute the byte offset where the string content starts in the file.
///
/// Relies on symmetric delimiters: `"..."` (1+1), `[[...]]` (2+2), `[=[...]=]` (3+3).
/// The `else` branch handles the edge case where `string_literals` stores trimmed
/// content for quoted strings (total_len == content_len), falling back to skip
/// a single opening quote character.
pub fn compute_content_start(content_len: usize, str_start: u32, str_end: u32) -> u32 {
    let total_len = (str_end - str_start) as usize;
    if total_len > content_len {
        let delimiter_total = total_len - content_len;
        let open_len = delimiter_total / 2;
        str_start + open_len as u32
    } else {
        str_start + 1
    }
}

/// Simple rule-based type inference for Lua expressions.
fn infer_expression_type(analysis: &AnalysisResult, tree: &SyntaxTree, table_idxs: &[TableIndex]) -> Option<ValueType> {
    // Tree structure: Block(root) → ReturnStatement → ExpressionList → expression
    // SyntaxNode::new_root IS the Block node.
    let root = SyntaxNode::new_root(tree);
    let ret_stmt = root.children().next()?;
    let expr_list = ret_stmt.children().next()?;
    let expr_node = expr_list.children().next()
        .unwrap_or(expr_list);
    infer_node_type(analysis, expr_node, table_idxs)
}

/// Infer the type of a syntax node within an expression context.
fn infer_node_type(analysis: &AnalysisResult, node: SyntaxNode<'_>, table_idxs: &[TableIndex]) -> Option<ValueType> {
    match node.kind() {
        SyntaxKind::UnaryExpression => {
            let op_token = node.children_with_tokens()
                .find_map(|c| c.into_token().filter(|t| matches!(t.kind(),
                    SyntaxKind::NotKeyword | SyntaxKind::Minus | SyntaxKind::Hash)))?;
            match op_token.kind() {
                SyntaxKind::NotKeyword => Some(ValueType::Boolean(None)),
                SyntaxKind::Minus => Some(ValueType::Number),
                SyntaxKind::Hash => Some(ValueType::Number),
                _ => None,
            }
        }
        SyntaxKind::BinaryExpression => {
            let mut children = node.children();
            let lhs = children.next()?;
            let rhs = children.next();

            let op_token = node.children_with_tokens()
                .filter_map(|c| c.into_token())
                .find(|t| matches!(t.kind(),
                    SyntaxKind::AndKeyword | SyntaxKind::OrKeyword |
                    SyntaxKind::EqualsBoolean | SyntaxKind::NotEqualsBoolean |
                    SyntaxKind::LessThan | SyntaxKind::LessThanOrEquals |
                    SyntaxKind::GreaterThan | SyntaxKind::GreaterThanOrEquals |
                    SyntaxKind::Plus | SyntaxKind::Minus |
                    SyntaxKind::Asterisk | SyntaxKind::Slash |
                    SyntaxKind::Modulo | SyntaxKind::Hat |
                    SyntaxKind::DoubleDot
                ));

            if let Some(op) = op_token {
                match op.kind() {
                    SyntaxKind::AndKeyword => {
                        rhs.and_then(|r| infer_node_type(analysis, r, table_idxs))
                    }
                    SyntaxKind::OrKeyword => {
                        let lhs_type = infer_node_type(analysis, lhs, table_idxs);
                        let rhs_type = rhs.and_then(|r| infer_node_type(analysis, r, table_idxs));
                        match (lhs_type, rhs_type) {
                            (Some(l), Some(r)) => Some(ValueType::make_union(vec![l, r])),
                            (Some(l), None) => Some(l),
                            (None, Some(r)) => Some(r),
                            (None, None) => None,
                        }
                    }
                    SyntaxKind::EqualsBoolean | SyntaxKind::NotEqualsBoolean |
                    SyntaxKind::LessThan | SyntaxKind::LessThanOrEquals |
                    SyntaxKind::GreaterThan | SyntaxKind::GreaterThanOrEquals => {
                        Some(ValueType::Boolean(None))
                    }
                    SyntaxKind::Plus | SyntaxKind::Minus |
                    SyntaxKind::Asterisk | SyntaxKind::Slash |
                    SyntaxKind::Modulo | SyntaxKind::Hat => {
                        Some(ValueType::Number)
                    }
                    SyntaxKind::DoubleDot => {
                        Some(ValueType::String(None))
                    }
                    _ => None,
                }
            } else {
                None
            }
        }
        SyntaxKind::GroupedExpression => {
            let inner = node.children().next()?;
            infer_node_type(analysis, inner, table_idxs)
        }
        SyntaxKind::NameRef | SyntaxKind::ExpressionList => {
            // NameRef wraps a single Name token; ExpressionList wraps expressions
            for child in node.children_with_tokens() {
                match child {
                    crate::syntax::NodeOrToken::Token(token) => match token.kind() {
                        SyntaxKind::Name => {
                            let word = token.text();
                            for &idx in table_idxs {
                                if let Some(fi) = analysis.get_field(idx, word) {
                                    if let Some(ref ann) = fi.annotation {
                                        return Some(ann.clone());
                                    }
                                    return analysis.resolve_expr_type(fi.expr);
                                }
                            }
                            return None;
                        }
                        SyntaxKind::NilKeyword => return Some(ValueType::Nil),
                        SyntaxKind::TrueKeyword => return Some(ValueType::Boolean(Some(true))),
                        SyntaxKind::FalseKeyword => return Some(ValueType::Boolean(Some(false))),
                        SyntaxKind::Number => return Some(ValueType::Number),
                        SyntaxKind::String => return Some(ValueType::String(None)),
                        _ => {}
                    }
                    crate::syntax::NodeOrToken::Node(child_node) => {
                        if let Some(t) = infer_node_type(analysis, child_node, table_idxs) {
                            return Some(t);
                        }
                    }
                }
            }
            None
        }
        SyntaxKind::ReturnStatement => {
            // "return <expr>" — descend into the expression list child
            node.children().next()
                .and_then(|child| infer_node_type(analysis, child, table_idxs))
        }
        _ => None,
    }
}

/// Simple assignability check for expression return type validation.
fn is_assignable(actual: &ValueType, expected: &ValueType) -> bool {
    if matches!(expected, ValueType::Any) || matches!(actual, ValueType::Any) {
        return true;
    }
    match (actual, expected) {
        (ValueType::Boolean(_), ValueType::Boolean(_)) => true,
        (ValueType::Number, ValueType::Number) => true,
        (ValueType::String(_), ValueType::String(None)) => true,
        (ValueType::String(Some(a)), ValueType::String(Some(b))) => a == b,
        (ValueType::Nil, ValueType::Nil) => true,
        (ValueType::Table(_), ValueType::Table(_)) => true,
        (ValueType::Function(_), ValueType::Function(_)) => true,
        // Union: actual is assignable if all members are assignable
        (ValueType::Union(members), _) => members.iter().all(|m| is_assignable(m, expected)),
        (_, ValueType::Union(members)) => members.iter().any(|m| is_assignable(actual, m)),
        // Opaque aliases: same rules as main type system
        (ValueType::OpaqueAlias(a, _), ValueType::OpaqueAlias(b, _)) if a != b => false,
        (_, ValueType::OpaqueAlias(_, inner)) => is_assignable(actual, inner),
        (ValueType::OpaqueAlias(_, inner), _) => is_assignable(inner, expected),
        _ => actual == expected,
    }
}
