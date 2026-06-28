use crate::analysis::AnalysisResult;
use crate::syntax::parser::Parser;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::{SyntaxKind, SyntaxNode};
use crate::types::{TableIndex, ValueType};
use super::{DiagnosticPass, WowDiagnostic};

pub struct ExpressionType;

impl DiagnosticPass for ExpressionType {
    fn run(&self, analysis: &AnalysisResult, _tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for (&expr_id, arg_info) in &analysis.ir.expression_args {
            // When the context type couldn't be fully resolved (an unbound generic
            // type-param member), `table_idxs` is only a partial view of the
            // available fields — suppress diagnostics to avoid false positives on
            // names the missing member would supply (hover/completion still work).
            if arg_info.context_incomplete {
                continue;
            }
            let table_idxs = &arg_info.table_idxs;
            let expected_return = &arg_info.return_type;
            let (str_start, str_end) = arg_info.str_range;
            let Some(raw_content) = analysis.ir.string_literals.get(&expr_id) else { continue };
            // string_literals stores content with all delimiters already stripped
            // (quotes, long brackets) — see strip_string_delimiters in lower_expression.rs.
            let content = raw_content.as_str();
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
                let name_type = |word: &str| -> Option<ValueType> {
                    table_idxs.iter().find_map(|&idx| {
                        let fi = analysis.get_field(idx, word)?;
                        fi.annotation.clone().or_else(|| analysis.resolve_expr_type(fi.expr))
                    })
                };
                let inferred = infer_expression_type(&expr_tree, &name_type);
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

/// Simple rule-based type inference for a Lua expression parsed as `return <expr>`.
///
/// `name_type(word)` resolves the type of a bare identifier (a context field of
/// the `expression<C, R>` class C); it returns `None` for unknown names. This is
/// shared between the `expression-type` diagnostic and generic `R` inference in
/// call resolution (`resolve_call.rs`), which supply different field lookups.
pub fn infer_expression_type(
    tree: &SyntaxTree,
    name_type: &dyn Fn(&str) -> Option<ValueType>,
) -> Option<ValueType> {
    // Tree structure: Block(root) → ReturnStatement → ExpressionList → expression
    // SyntaxNode::new_root IS the Block node.
    let root = SyntaxNode::new_root(tree);
    let ret_stmt = root.children().next()?;
    let expr_list = ret_stmt.children().next()?;
    let expr_node = expr_list.children().next()
        .unwrap_or(expr_list);
    infer_node_type(expr_node, name_type)
}

/// Infer the type of a syntax node within an expression context.
fn infer_node_type(node: SyntaxNode<'_>, name_type: &dyn Fn(&str) -> Option<ValueType>) -> Option<ValueType> {
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
                        rhs.and_then(|r| infer_node_type(r, name_type))
                    }
                    SyntaxKind::OrKeyword => {
                        let lhs_type = infer_node_type(lhs, name_type);
                        let rhs_type = rhs.and_then(|r| infer_node_type(r, name_type));
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
            infer_node_type(inner, name_type)
        }
        SyntaxKind::NameRef | SyntaxKind::ExpressionList => {
            // NameRef wraps a single Name token; ExpressionList wraps expressions
            for child in node.children_with_tokens() {
                match child {
                    crate::syntax::NodeOrToken::Token(token) => match token.kind() {
                        SyntaxKind::Name => {
                            return name_type(token.text());
                        }
                        SyntaxKind::NilKeyword => return Some(ValueType::Nil),
                        SyntaxKind::TrueKeyword => return Some(ValueType::Boolean(Some(true))),
                        SyntaxKind::FalseKeyword => return Some(ValueType::Boolean(Some(false))),
                        SyntaxKind::Number => return Some(ValueType::Number),
                        SyntaxKind::String => return Some(ValueType::String(None)),
                        _ => {}
                    }
                    crate::syntax::NodeOrToken::Node(child_node) => {
                        if let Some(t) = infer_node_type(child_node, name_type) {
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
                .and_then(|child| infer_node_type(child, name_type))
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
