use crate::analysis::AnalysisResult;
use crate::ast::*;
use crate::syntax::SyntaxKind;
use crate::syntax::{SyntaxNode, NodeOrToken};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct AstChecks;

impl DiagnosticPass for AstChecks {
    fn visit_node(&self, node: SyntaxNode<'_>, _analysis: &AnalysisResult, diags: &mut Vec<WowDiagnostic>) {
        match node.kind() {
            SyntaxKind::Block => {
                if let Some(block) = Block::cast(node) {
                    check_block_diagnostics(diags, block);
                }
            }
            SyntaxKind::LocalAssignStatement => {
                if let Some(assign) = LocalAssign::cast(node) {
                    check_assignment_balance_local(diags, assign);
                }
            }
            SyntaxKind::AssignStatement => {
                if let Some(assign) = Assign::cast(node) {
                    check_assignment_balance_nonlocal(diags, assign);
                }
            }
            SyntaxKind::WhileLoop | SyntaxKind::RepeatUntilLoop => {
                if let Some(block) = node.children().find_map(Block::cast)
                    && block_is_empty(&block)
                {
                    let r = node.text_range();
                    super::EMPTY_BLOCK.emit(
                        diags,
                        "empty block".to_string(),
                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                    );
                }
            }
            SyntaxKind::ForCountLoop => {
                if let Some(for_loop) = ForCountLoop::cast(node) {
                    check_for_count_loop(diags, for_loop);
                }
            }
            SyntaxKind::ForInLoop => {
                if let Some(for_in) = ForInLoop::cast(node)
                    && let Some(block) = for_in.block()
                    && block_is_empty(&block)
                {
                    let r = for_in.syntax().text_range();
                    super::EMPTY_BLOCK.emit(
                        diags,
                        "empty block".to_string(),
                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                    );
                }
            }
            SyntaxKind::IfChain => {
                if let Some(if_chain) = IfChain::cast(node) {
                    check_if_chain_empty_blocks(diags, if_chain);
                }
            }
            _ => {}
        }
    }
}

fn check_block_diagnostics(
    diags: &mut Vec<WowDiagnostic>,
    block: Block<'_>,
) {
    let block_node = block.syntax();
    let statements = block.statements();

    let mut saw_break = false;
    for child in block_node.children_with_tokens() {
        if let NodeOrToken::Token(tok) = &child {
            if tok.kind() == SyntaxKind::BreakKeyword {
                saw_break = true;
            }
        } else if let NodeOrToken::Node(ref n) = child
            && saw_break && Statement::cast(*n).is_some() {
                let r = n.text_range();
                super::CODE_AFTER_BREAK.emit(
                    diags,
                    "unreachable code after break statement".to_string(),
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
                break;
            }
    }

    for (i, stmt) in statements.iter().enumerate() {
        if matches!(stmt, Statement::Return(_)) && i + 1 < statements.len() {
            let next_stmt = &statements[i + 1];
            let r = next_stmt.syntax().text_range();
            super::UNREACHABLE_CODE.emit(
                diags,
                "unreachable code after return statement".to_string(),
                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
            );
        }

        if i + 1 == statements.len()
            && let Statement::Return(ret) = stmt
        {
            let has_values = ret.expression_list()
                .is_some_and(|el| !el.expressions().is_empty());
            let is_fn_top_block = block_node.parent()
                .is_some_and(|p| p.kind() == SyntaxKind::FunctionDefinition);
            if !has_values && is_fn_top_block {
                let r = ret.syntax().text_range();
                super::REDUNDANT_RETURN.emit(
                    diags,
                    "redundant return statement at end of function".to_string(),
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
        }
    }
}

fn check_assignment_balance_local(
    diags: &mut Vec<WowDiagnostic>,
    assign: LocalAssign<'_>,
) {
    let Some(name_list) = assign.name_list() else { return };
    let names = name_list.names();
    let expressions = assign
        .expression_list()
        .map(|el| el.expressions())
        .unwrap_or_default();
    let last_is_multi = matches!(
        expressions.last(),
        Some(Expression::FunctionCall(_)) | Some(Expression::VarArgs(_))
    );
    if !last_is_multi && !expressions.is_empty() {
        if expressions.len() > names.len() {
            if let Some(extra) = expressions.get(names.len()) {
                let r = extra.syntax().text_range();
                super::REDUNDANT_VALUE.emit(
                    diags,
                    format!("{} value(s) assigned to {} variable(s)", expressions.len(), names.len()),
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
        } else if names.len() > expressions.len() {
            let r = assign.syntax().text_range();
            super::UNBALANCED_ASSIGNMENTS.emit(
                diags,
                format!("{} variable(s) but only {} value(s)", names.len(), expressions.len()),
                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
            );
        }
    }
}

fn check_assignment_balance_nonlocal(
    diags: &mut Vec<WowDiagnostic>,
    assign: Assign<'_>,
) {
    let Some(var_list) = assign.variable_list() else { return };
    let identifiers = var_list.identifiers();
    let expressions = assign
        .expression_list()
        .map(|el| el.expressions())
        .unwrap_or_default();
    let last_is_multi = matches!(
        expressions.last(),
        Some(Expression::FunctionCall(_)) | Some(Expression::VarArgs(_))
    );
    if !last_is_multi && !expressions.is_empty() {
        if expressions.len() > identifiers.len() {
            if let Some(extra) = expressions.get(identifiers.len()) {
                let r = extra.syntax().text_range();
                super::REDUNDANT_VALUE.emit(
                    diags,
                    format!("{} value(s) assigned to {} variable(s)", expressions.len(), identifiers.len()),
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
        } else if identifiers.len() > expressions.len() {
            let r = assign.syntax().text_range();
            super::UNBALANCED_ASSIGNMENTS.emit(
                diags,
                format!("{} variable(s) but only {} value(s)", identifiers.len(), expressions.len()),
                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
            );
        }
    }
}

fn check_for_count_loop(
    diags: &mut Vec<WowDiagnostic>,
    for_loop: ForCountLoop<'_>,
) {
    if let Some(block) = for_loop.block()
        && block_is_empty(&block)
    {
        let r = for_loop.syntax().text_range();
        super::EMPTY_BLOCK.emit(
            diags,
            "empty block".to_string(),
            u32::from(r.start()) as usize, u32::from(r.end()) as usize,
        );
    }

    let Some(expr_list) = for_loop.expression_list() else { return };
    let exprs = expr_list.expressions();
    if exprs.len() < 2 { return; }
    let start_val = expr_literal_number(&exprs[0]);
    let end_val = expr_literal_number(&exprs[1]);
    let step_val = if exprs.len() >= 3 {
        expr_literal_number(&exprs[2])
    } else {
        None
    };
    let (Some(sv), Some(ev)) = (start_val, end_val) else { return };
    let step = step_val.unwrap_or(1.0);
    let should_warn = if step == 0.0 {
        step_val.is_some() && sv != ev
    } else {
        let counting_down = sv > ev;
        let step_positive = step > 0.0;
        (counting_down && step_positive) || (!counting_down && sv != ev && !step_positive)
    };
    if !should_warn { return; }
    let msg = if step_val.is_none() {
        format!("loop from {} to {} will not execute (implicit step is 1; use -1)", sv, ev)
    } else if step == 0.0 {
        format!("loop from {} to {} with step 0 will loop forever", sv, ev)
    } else {
        format!("loop from {} to {} with step {} will not execute", sv, ev, step)
    };
    let br = for_loop.syntax().text_range();
    super::COUNT_DOWN_LOOP.emit(
        diags,
        msg,
        u32::from(br.start()) as usize,
        u32::from(br.end()) as usize,
    );
}

fn check_if_chain_empty_blocks(
    diags: &mut Vec<WowDiagnostic>,
    if_chain: IfChain<'_>,
) {
    for branch in if_chain.if_branches() {
        if let Some(inner_block) = branch.block()
            && block_is_empty(&inner_block)
        {
            let r = branch.syntax().text_range();
            super::EMPTY_BLOCK.emit(
                diags,
                "empty block".to_string(),
                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
            );
        }
    }
    if let Some(else_branch) = if_chain.else_branch()
        && let Some(inner_block) = else_branch.block()
        && block_is_empty(&inner_block)
    {
        let r = else_branch.syntax().text_range();
        super::EMPTY_BLOCK.emit(
            diags,
            "empty block".to_string(),
            u32::from(r.start()) as usize, u32::from(r.end()) as usize,
        );
    }
}

fn block_is_empty(block: &Block<'_>) -> bool {
    if !block.statements().is_empty() { return false; }
    for child in block.syntax().children_with_tokens() {
        if let NodeOrToken::Token(tok) = &child
            && (tok.kind() == SyntaxKind::BreakKeyword || tok.kind() == SyntaxKind::Comment)
        {
            return false;
        }
    }
    true
}

fn expr_literal_number(expr: &Expression<'_>) -> Option<f64> {
    match expr {
        Expression::Literal(lit) => {
            lit.get_number().and_then(|s| s.trim().parse::<f64>().ok())
        }
        Expression::UnaryExpression(unary) => {
            if unary.kind() == Operator::Subtract {
                let terms = unary.get_terms();
                if let Some(Expression::Literal(lit)) = terms.first() {
                    return lit.get_number().and_then(|s| s.trim().parse::<f64>().ok()).map(|v| -v);
                }
            }
            None
        }
        _ => None,
    }
}
