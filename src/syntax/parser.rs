use crate::syntax::lexer::{self, LexError};
use crate::syntax::SyntaxKind as SK;
use crate::syntax::tree::{TreeBuilder, SyntaxTree, Checkpoint};

#[derive(Debug, Clone, Copy)]
struct Tok {
    kind: SK,
    start: u32,
    end: u32,
}

pub struct Parser<'a> {
    text: &'a str,
    lexer: lexer::Lexer<'a>,
    builder: TreeBuilder,
    peek: Option<Tok>,
}

impl<'a> Parser<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            lexer: lexer::Lexer::new(text),
            builder: TreeBuilder::new(text.to_string()),
            peek: None,
        }
    }

    pub fn parse(mut self) -> SyntaxTree {
        self.parse_block(None);
        self.builder.finish()
    }

    // ── Token management ──

    fn peek(&mut self) -> Option<Tok> {
        if self.peek.is_some() {
            return self.peek;
        }
        let raw = self.lexer.next_token()?;
        // Report lexer errors
        if let Some(err) = raw.error {
            match err {
                LexError::InvalidNumber => {
                    self.builder.error(raw.start, raw.end,
                        format!("malformed number `{}`", &self.text[raw.start as usize..raw.end as usize]));
                }
                LexError::UnterminatedString => {
                    self.builder.error(raw.start, raw.end, "unterminated string".to_string());
                }
                LexError::UnterminatedComment => {
                    self.builder.error(raw.start, self.text.len() as u32,
                        "unterminated comment, expected closing `]]`".to_string());
                }
            }
        }
        let tok = Tok { kind: raw.kind, start: raw.start, end: raw.end };
        self.peek = Some(tok);
        Some(tok)
    }

    /// Consume the peeked token without emitting it.
    fn advance(&mut self) -> Option<Tok> {
        let tok = self.peek()?;
        self.peek = None;
        Some(tok)
    }

    /// Consume and emit the next token.
    fn bump(&mut self) -> Option<Tok> {
        let tok = self.advance()?;
        self.builder.token(tok.kind, tok.start, tok.end);
        Some(tok)
    }

    /// Bump only if the next non-trivia token matches.
    fn bump_if(&mut self, kind: SK) -> bool {
        if self.at(kind) { self.bump(); true } else { false }
    }

    /// Check if next non-trivia token matches.
    fn at(&mut self, kind: SK) -> bool {
        self.skip_trivia();
        self.peek().is_some_and(|t| t.kind == kind)
    }

    /// Eat and emit all trivia tokens.
    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(tok) if tok.kind.is_trivia() => { self.bump(); }
                _ => break,
            }
        }
    }

    fn current_pos(&mut self) -> u32 {
        self.peek().map_or(self.text.len() as u32, |t| t.start)
    }

    fn error(&mut self, start: u32, end: u32, msg: String) {
        self.builder.error(start, end, msg);
    }

    fn error_here(&mut self, msg: String) {
        let pos = self.current_pos();
        self.builder.error(pos, pos, msg);
    }

    fn text_at(&self, start: u32, end: u32) -> &str {
        &self.text[start as usize..end as usize]
    }

    fn checkpoint(&self) -> Checkpoint {
        self.builder.checkpoint()
    }

    // ── Block ──

    fn parse_block(&mut self, terminator: Option<SK>) {
        self.builder.start_node(SK::Block);
        self.skip_trivia();
        loop {
            let Some(tok) = self.peek() else {
                if terminator.is_some() {
                    self.error_here("block is not closed, expected `end`".to_string());
                }
                break;
            };
            // Break on the expected terminator, or on any block-closing keyword from an
            // outer context (end / else / elseif / until).  This prevents a cascade where,
            // e.g., a `while` body missing its `end` consumes an `until` that belongs to
            // an enclosing `repeat`, or a `repeat` body missing its `until` is entered by
            // a stray `end` that should close a surrounding block.
            if let Some(term) = terminator
                && (tok.kind == term || is_block_terminator(tok.kind))
            {
                break;
            }
            self.parse_statement();
            self.skip_trivia();
        }
        self.builder.finish_node();
    }

    // ── Statements ──

    fn parse_statement(&mut self) {
        let Some(tok) = self.peek() else { return };
        match tok.kind {
            SK::DoKeyword => self.parse_do_block(),
            SK::WhileKeyword => self.parse_while_loop(),
            SK::RepeatKeyword => self.parse_repeat_loop(),
            SK::IfKeyword => self.parse_if_chain(),
            SK::ForKeyword => self.parse_for_loop(),
            SK::LocalKeyword => self.parse_local(),
            SK::FunctionKeyword => self.parse_function_def_stmt(),
            SK::ReturnKeyword => self.parse_return(),
            SK::BreakKeyword => { self.bump(); }
            SK::Semicolon => { self.bump(); }
            SK::Name | SK::LeftBracket => self.parse_expr_statement(),
            // Stray block terminators — no matching opener at this level.
            // Consume the single token with an error so the parse_block loop can
            // continue (important at top level where the loop has no terminator
            // check and would otherwise spin forever on the same token).
            SK::EndKeyword | SK::ElseKeyword | SK::ElseIfKeyword | SK::UntilKeyword => {
                let tok = self.bump().unwrap();
                self.error(tok.start, tok.end, format!("unexpected `{}`", self.text_at(tok.start, tok.end)));
            }
            _ => {
                // Skip-ahead recovery: consume all non-statement tokens in one pass
                // so that the next well-formed statement can be parsed cleanly.
                // This collapses a run of garbage tokens into a single error span
                // instead of generating one error per token.
                let start = self.current_pos();
                loop {
                    match self.peek() {
                        None => break,
                        Some(t) if is_stmt_starter(t.kind) || is_block_terminator(t.kind) => break,
                        _ => { self.bump(); }
                    }
                }
                let end = self.current_pos();
                self.error(start, end, "unexpected token".to_string());
            }
        }
    }

    fn parse_do_block(&mut self) {
        self.builder.start_node(SK::DoBlock);
        let start = self.bump().unwrap();
        self.parse_block(Some(SK::EndKeyword));
        self.skip_trivia();
        if !self.bump_if(SK::EndKeyword) {
            self.error(start.start, start.end, "`do` is not closed, expected `end`".to_string());
        }
        self.builder.finish_node();
    }

    fn parse_while_loop(&mut self) {
        self.builder.start_node(SK::WhileLoop);
        let start = self.bump().unwrap();
        self.skip_trivia();
        self.builder.start_node(SK::Condition);
        if !self.parse_expression() {
            self.error(start.start, start.end, "expected expression after `while`".to_string());
        }
        self.builder.finish_node();
        self.skip_trivia();
        if self.bump_if(SK::DoKeyword) {
            self.parse_block(Some(SK::EndKeyword));
            self.skip_trivia();
            if !self.bump_if(SK::EndKeyword) {
                self.error(start.start, start.end, "`while` is not closed, expected `end`".to_string());
            }
        } else {
            self.error_here("expected `do`".to_string());
        }
        self.builder.finish_node();
    }

    fn parse_repeat_loop(&mut self) {
        self.builder.start_node(SK::RepeatUntilLoop);
        let start = self.bump().unwrap();
        self.parse_block(Some(SK::UntilKeyword));
        self.skip_trivia();
        if self.bump_if(SK::UntilKeyword) {
            self.skip_trivia();
            if !self.parse_expression() {
                self.error_here("expected expression after `until`".to_string());
            }
        } else {
            self.error(start.start, start.end, "`repeat` is not closed, expected `until`".to_string());
        }
        self.builder.finish_node();
    }

    fn parse_if_chain(&mut self) {
        self.builder.start_node(SK::IfChain);
        self.parse_if_branch();
        self.skip_trivia();
        while self.at(SK::ElseIfKeyword) {
            self.parse_if_branch();
            self.skip_trivia();
        }
        if self.at(SK::ElseKeyword) {
            self.builder.start_node(SK::ElseBranch);
            self.bump();
            self.parse_block(Some(SK::EndKeyword));
            self.builder.finish_node();
            self.skip_trivia();
        }
        if !self.bump_if(SK::EndKeyword) {
            self.error_here("`if` is not closed, expected `end`".to_string());
        }
        self.builder.finish_node();
    }

    fn parse_if_branch(&mut self) {
        self.builder.start_node(SK::IfBranch);
        let start = self.bump().unwrap(); // `if` or `elseif`
        self.skip_trivia();
        self.builder.start_node(SK::Condition);
        if !self.parse_expression() {
            self.error(start.start, start.end, "expected expression after condition".to_string());
        }
        self.builder.finish_node();
        self.skip_trivia();
        if !self.bump_if(SK::ThenKeyword) {
            self.error_here("expected `then` after condition".to_string());
            self.builder.finish_node();
            return;
        }
        self.parse_block(Some(SK::EndKeyword));
        self.builder.finish_node();
    }

    fn parse_for_loop(&mut self) {
        // Peek ahead past `for NAME` to decide count vs in.
        // Consume tokens without emitting, then emit inside the right node.
        let for_tok = self.advance().unwrap();
        let mut trivia1 = Vec::new();
        while let Some(t) = self.peek() {
            if t.kind.is_trivia() { trivia1.push(self.advance().unwrap()); } else { break; }
        }
        let Some(name_tok) = self.peek() else {
            self.builder.start_node(SK::ForCountLoop);
            self.builder.token(for_tok.kind, for_tok.start, for_tok.end);
            self.emit_trivia(&trivia1);
            self.error_here("expected name after `for`".to_string());
            self.builder.finish_node();
            return;
        };
        if name_tok.kind != SK::Name {
            self.builder.start_node(SK::ForCountLoop);
            self.builder.token(for_tok.kind, for_tok.start, for_tok.end);
            self.emit_trivia(&trivia1);
            let bad = self.bump().unwrap();
            self.error(bad.start, bad.end, format!("expected name, found `{}`", self.text_at(bad.start, bad.end)));
            self.builder.finish_node();
            return;
        }
        let name = self.advance().unwrap();
        let mut trivia2 = Vec::new();
        while let Some(t) = self.peek() {
            if t.kind.is_trivia() { trivia2.push(self.advance().unwrap()); } else { break; }
        }
        let is_count = self.peek().is_some_and(|t| t.kind == SK::Assign);
        if is_count {
            self.builder.start_node(SK::ForCountLoop);
            self.builder.token(for_tok.kind, for_tok.start, for_tok.end);
            self.emit_trivia(&trivia1);
            self.builder.token(SK::Name, name.start, name.end);
            self.emit_trivia(&trivia2);
            self.bump(); // `=`
            self.skip_trivia();
            self.builder.start_node(SK::ExpressionList);
            self.parse_expression_list();
            self.builder.finish_node();
            self.skip_trivia();
            if self.bump_if(SK::DoKeyword) {
                self.parse_block(Some(SK::EndKeyword));
                self.skip_trivia();
                if !self.bump_if(SK::EndKeyword) {
                    self.error(for_tok.start, for_tok.end, "`for` is not closed, expected `end`".to_string());
                }
            } else {
                self.error_here("expected `do`".to_string());
            }
            self.builder.finish_node();
        } else {
            self.builder.start_node(SK::ForInLoop);
            self.builder.token(for_tok.kind, for_tok.start, for_tok.end);
            self.emit_trivia(&trivia1);
            self.builder.start_node(SK::NameList);
            self.builder.token(SK::Name, name.start, name.end);
            self.emit_trivia(&trivia2);
            while self.at(SK::Comma) {
                self.bump();
                self.skip_trivia();
                if self.at(SK::Name) { self.bump(); } else { self.error_here("expected name".to_string()); break; }
                self.skip_trivia();
            }
            self.builder.finish_node();
            self.skip_trivia();
            if !self.bump_if(SK::InKeyword) {
                self.error_here("expected `in` after variable list".to_string());
            } else {
                self.builder.start_node(SK::ExpressionList);
                self.parse_expression_list();
                self.builder.finish_node();
            }
            self.skip_trivia();
            if self.bump_if(SK::DoKeyword) {
                self.parse_block(Some(SK::EndKeyword));
                self.skip_trivia();
                if !self.bump_if(SK::EndKeyword) {
                    self.error(for_tok.start, for_tok.end, "`for` is not closed, expected `end`".to_string());
                }
            } else {
                self.error_here("expected `do`".to_string());
            }
            self.builder.finish_node();
        }
    }

    fn parse_local(&mut self) {
        let local_tok = self.advance().unwrap();
        let mut trivia = Vec::new();
        while let Some(t) = self.peek() {
            if t.kind.is_trivia() { trivia.push(self.advance().unwrap()); } else { break; }
        }
        let Some(next) = self.peek() else {
            self.builder.token(local_tok.kind, local_tok.start, local_tok.end);
            self.emit_trivia(&trivia);
            self.error_here("expected name or `function` after `local`".to_string());
            return;
        };
        if next.kind == SK::FunctionKeyword {
            self.builder.start_node(SK::FunctionDefinition);
            self.builder.token(local_tok.kind, local_tok.start, local_tok.end);
            self.emit_trivia(&trivia);
            self.bump(); // `function`
            self.skip_trivia();
            if self.at(SK::Name) { self.bump(); }
            else { self.error_here("expected function name".to_string()); }
            self.parse_param_list();
            self.parse_block(Some(SK::EndKeyword));
            self.skip_trivia();
            if !self.bump_if(SK::EndKeyword) {
                self.error(local_tok.start, local_tok.end, "`function` is not closed, expected `end`".to_string());
            }
            self.builder.finish_node();
        } else if next.kind == SK::Name {
            self.builder.start_node(SK::LocalAssignStatement);
            self.builder.token(local_tok.kind, local_tok.start, local_tok.end);
            self.emit_trivia(&trivia);
            self.parse_name_list();
            self.skip_trivia();
            if self.at(SK::Assign) {
                self.bump();
                self.builder.start_node(SK::ExpressionList);
                self.parse_expression_list();
                self.builder.finish_node();
            }
            self.builder.finish_node();
        } else {
            self.builder.token(local_tok.kind, local_tok.start, local_tok.end);
            self.emit_trivia(&trivia);
            let bad = self.bump().unwrap();
            self.error(bad.start, bad.end, format!("unexpected `{}`", self.text_at(bad.start, bad.end)));
        }
    }

    fn parse_function_def_stmt(&mut self) {
        self.builder.start_node(SK::FunctionDefinition);
        let start = self.bump().unwrap(); // `function`
        self.skip_trivia();
        if self.at(SK::Name) {
            self.parse_function_name();
        } else {
            self.error_here("expected function name or parameters".to_string());
        }
        self.parse_param_list();
        self.parse_block(Some(SK::EndKeyword));
        self.skip_trivia();
        if !self.bump_if(SK::EndKeyword) {
            self.error(start.start, start.end, "`function` is not closed, expected `end`".to_string());
        }
        self.builder.finish_node();
    }

    /// Parse function name: NAME [.NAME]* [:NAME]
    /// Simple name: emitted as bare Name token.
    /// Dotted/colon name: wrapped in DotAccess/MethodCall-like Identifier node
    /// to match old parser behavior (consumer code expects Identifier child).
    fn parse_function_name(&mut self) {
        // Check if this is a multi-part name by peeking past the first name
        let cp = self.checkpoint();
        self.bump(); // first Name
        self.skip_trivia();

        let has_more = self.peek().is_some_and(|t| t.kind == SK::Dot || t.kind == SK::Colon);
        if !has_more {
            return; // Simple name, no wrapping needed
        }

        // Multi-part name: wrap everything in an Identifier-equivalent node.
        // We use DotAccess as the wrapper kind, which maps to SyntaxKind::DotAccess
        // and is accepted by Identifier::cast() in ast.rs.
        // Use start_node_at to retroactively wrap the already-emitted first Name.
        self.builder.start_node_at(cp, SK::DotAccess);
        while let Some(tok) = self.peek() {
            match tok.kind {
                SK::Dot => {
                    self.bump();
                    self.skip_trivia();
                    if self.at(SK::Name) { self.bump(); }
                    else { self.error_here("expected name after `.`".to_string()); break; }
                    self.skip_trivia();
                }
                SK::Colon => {
                    self.bump();
                    self.skip_trivia();
                    if self.at(SK::Name) { self.bump(); }
                    else { self.error_here("expected name after `:`".to_string()); }
                    break;
                }
                _ => break,
            }
        }
        self.builder.finish_node();
    }

    fn parse_return(&mut self) {
        self.builder.start_node(SK::ReturnStatement);
        self.bump(); // `return`
        self.skip_trivia();
        if let Some(tok) = self.peek()
            && !matches!(tok.kind, SK::EndKeyword | SK::ElseKeyword | SK::ElseIfKeyword | SK::UntilKeyword) {
                self.builder.start_node(SK::ExpressionList);
                self.parse_expression_list();
                self.builder.finish_node();
            }
        self.builder.finish_node();
    }

    /// Expression statement: function call or assignment.
    /// Uses checkpoint to wrap in AssignStatement if `=` follows.
    fn parse_expr_statement(&mut self) {
        let cp = self.checkpoint();

        // Parse first suffixed expression
        self.parse_suffixed_expression();
        self.skip_trivia();

        let Some(next) = self.peek() else { return };

        if next.kind == SK::Comma || next.kind == SK::Assign {
            // Multi-assignment or assignment
            // Wrap what we've parsed so far in VariableList
            self.builder.start_node_at(cp, SK::VariableList);
            while self.at(SK::Comma) {
                self.bump();
                self.skip_trivia();
                self.parse_suffixed_expression();
                self.skip_trivia();
            }
            self.builder.finish_node(); // VariableList

            // Now wrap VariableList + rhs in AssignStatement
            self.builder.start_node_at(cp, SK::AssignStatement);
            self.skip_trivia();
            if self.at(SK::Assign) {
                self.bump();
                self.builder.start_node(SK::ExpressionList);
                self.parse_expression_list();
                self.builder.finish_node();
            } else {
                self.error_here("expected `=` or function call".to_string());
            }
            self.builder.finish_node(); // AssignStatement
        }
        // If neither comma nor assign, it was a bare function call — already emitted.
    }

    // ── Expressions ──

    /// Parse an expression. Returns true if something was parsed.
    fn parse_expression(&mut self) -> bool {
        self.parse_expr_bp(0)
    }

    /// Pratt parser with checkpoint-based wrapping.
    fn parse_expr_bp(&mut self, min_bp: u8) -> bool {
        self.skip_trivia();

        let cp = self.checkpoint();

        // Prefix: unary operators or atom
        let Some(tok) = self.peek() else { return false };
        match tok.kind {
            SK::Minus | SK::NotKeyword | SK::Hash => {
                self.builder.start_node(SK::UnaryExpression);
                self.bump();
                self.skip_trivia();
                if !self.parse_expr_bp(UNARY_BP) {
                    self.error_here("expected expression after unary operator".to_string());
                }
                self.builder.finish_node();
            }
            _ => {
                if !self.parse_primary_expression() {
                    return false;
                }
            }
        }

        // Infix: binary operators
        loop {
            self.skip_trivia();
            let Some(tok) = self.peek() else { break };
            let Some((left_bp, right_bp)) = infix_binding_power(tok.kind) else { break };
            if left_bp < min_bp { break; }

            // Wrap the left operand + operator + right operand in BinaryExpr
            self.builder.start_node_at(cp, SK::BinaryExpression);
            self.bump(); // operator
            self.skip_trivia();
            if !self.parse_expr_bp(right_bp) {
                self.error_here("expected expression after operator".to_string());
            }
            self.builder.finish_node();
        }

        true
    }

    /// Parse a primary expression (atom) followed by any suffixes.
    fn parse_primary_expression(&mut self) -> bool {
        let Some(tok) = self.peek() else { return false };
        match tok.kind {
            SK::Number | SK::String => {
                self.builder.start_node(SK::Literal);
                self.bump();
                self.builder.finish_node();
                // String literals can be call args: foo "hello"
                self.parse_suffixes();
                true
            }
            SK::TrueKeyword | SK::FalseKeyword | SK::NilKeyword => {
                self.builder.start_node(SK::Literal);
                self.bump();
                self.builder.finish_node();
                true
            }
            SK::TripleDot => {
                self.builder.start_node(SK::Literal);
                self.bump();
                self.builder.finish_node();
                true
            }
            SK::LeftBracket => {
                self.builder.start_node(SK::GroupedExpression);
                self.bump();
                self.skip_trivia();
                self.parse_expression();
                self.skip_trivia();
                if !self.bump_if(SK::RightBracket) {
                    self.error_here("expected `)`".to_string());
                }
                self.builder.finish_node();
                self.parse_suffixes();
                true
            }
            SK::LeftCurlyBracket => {
                self.parse_table_constructor();
                true
            }
            SK::FunctionKeyword => {
                self.builder.start_node(SK::FunctionDefinition);
                self.bump();
                self.parse_param_list();
                self.parse_block(Some(SK::EndKeyword));
                self.skip_trivia();
                if !self.bump_if(SK::EndKeyword) {
                    self.error_here("`function` is not closed, expected `end`".to_string());
                }
                self.builder.finish_node();
                true
            }
            SK::Name => {
                self.builder.start_node(SK::NameRef);
                self.bump();
                self.builder.finish_node();
                self.parse_suffixes();
                true
            }
            _ => false,
        }
    }

    /// Parse suffixes: `.field`, `:method(args)`, `[key]`, `(args)`, `"str"`, `{tbl}`.
    /// Uses checkpoint to retroactively wrap the base expression.
    fn parse_suffixes(&mut self) {
        loop {
            // Checkpoint BEFORE the trivia so the base expression is captured.
            // We need the checkpoint at the level of the base expression.
            // Since the base was already emitted as the last child of the current node,
            // we take a checkpoint that includes it.
            let cp = self.checkpoint_before_last();
            self.skip_trivia();
            let Some(tok) = self.peek() else { break };

            match tok.kind {
                SK::Dot => {
                    // Peek ahead: only create DotAccess if a Name follows
                    // on the same line. For incomplete `obj.` at end of line,
                    // emit the dot and break (error recovery for completions).
                    let dot_tok = self.advance().unwrap();
                    // Only skip whitespace (NOT newlines/comments) after dot
                    let mut ws_after_dot = Vec::new();
                    while let Some(t) = self.peek() {
                        if t.kind == SK::Whitespace { ws_after_dot.push(self.advance().unwrap()); }
                        else { break; }
                    }
                    if self.peek().is_some_and(|t| t.kind == SK::Name) {
                        self.builder.start_node_at(cp, SK::DotAccess);
                        self.builder.token(dot_tok.kind, dot_tok.start, dot_tok.end);
                        for t in &ws_after_dot { self.builder.token(t.kind, t.start, t.end); }
                        self.bump(); // Name
                        self.builder.finish_node();
                    } else {
                        // Incomplete dot access — emit dot token and break
                        self.builder.token(dot_tok.kind, dot_tok.start, dot_tok.end);
                        for t in &ws_after_dot { self.builder.token(t.kind, t.start, t.end); }
                        break;
                    }
                }
                SK::Colon => {
                    self.builder.start_node_at(cp, SK::MethodCall);
                    self.bump(); // `:`
                    self.skip_trivia();
                    if self.at(SK::Name) { self.bump(); }
                    else {
                        self.error_here("expected name after `:`".to_string());
                        self.builder.finish_node();
                        break;
                    }
                    self.skip_trivia();
                    // Only parse call args if they're present (for error recovery
                    // with incomplete input like `:method` without parentheses)
                    if self.peek().is_some_and(|t| matches!(t.kind, SK::LeftBracket | SK::LeftCurlyBracket | SK::String)) {
                        self.parse_call_args();
                    }
                    self.builder.finish_node();
                }
                SK::LeftSquareBracket => {
                    self.builder.start_node_at(cp, SK::BracketAccess);
                    self.bump(); // `[`
                    self.skip_trivia();
                    self.parse_expression();
                    self.skip_trivia();
                    if !self.bump_if(SK::RightSquareBracket) {
                        self.error_here("expected `]`".to_string());
                    }
                    self.builder.finish_node();
                }
                SK::LeftBracket | SK::LeftCurlyBracket | SK::String => {
                    self.builder.start_node_at(cp, SK::FunctionCall);
                    self.parse_call_args();
                    self.builder.finish_node();
                }
                _ => break,
            }
        }
    }

    /// Get a checkpoint positioned just before the last child of the current node.
    /// This allows wrapping the most recently emitted child in a new node.
    fn checkpoint_before_last(&self) -> Checkpoint {
        self.builder.checkpoint_before_last()
    }

    /// Parse function call arguments.
    fn parse_call_args(&mut self) {
        self.skip_trivia();
        let Some(tok) = self.peek() else { return };
        self.builder.start_node(SK::ArgumentList);
        match tok.kind {
            SK::LeftBracket => {
                self.bump();
                self.skip_trivia();
                if !self.at(SK::RightBracket) {
                    self.parse_expression_list();
                }
                self.skip_trivia();
                if !self.bump_if(SK::RightBracket) {
                    self.error_here("expected `)`".to_string());
                }
            }
            SK::String => { self.bump(); }
            SK::LeftCurlyBracket => { self.parse_table_constructor(); }
            _ => { self.error_here("expected function arguments".to_string()); }
        }
        self.builder.finish_node();
    }

    /// Parse a suffixed expression for statement context.
    fn parse_suffixed_expression(&mut self) {
        self.skip_trivia();
        let Some(tok) = self.peek() else { return };
        match tok.kind {
            SK::Name => {
                self.builder.start_node(SK::NameRef);
                self.bump();
                self.builder.finish_node();
                self.parse_suffixes();
            }
            SK::LeftBracket => {
                self.builder.start_node(SK::GroupedExpression);
                self.bump();
                self.skip_trivia();
                self.parse_expression();
                self.skip_trivia();
                if !self.bump_if(SK::RightBracket) {
                    self.error_here("expected `)`".to_string());
                }
                self.builder.finish_node();
                self.parse_suffixes();
            }
            _ => {
                self.error_here("expected name or `(`".to_string());
            }
        }
    }

    // ── Lists ──

    fn parse_expression_list(&mut self) -> bool {
        self.skip_trivia();
        if !self.parse_expression() { return false; }
        loop {
            self.skip_trivia();
            if !self.at(SK::Comma) { break; }
            self.bump();
            self.skip_trivia();
            if !self.parse_expression() {
                self.error_here("expected expression after `,`".to_string());
                break;
            }
        }
        true
    }

    fn parse_name_list(&mut self) {
        self.builder.start_node(SK::NameList);
        if self.at(SK::Name) { self.bump(); }
        loop {
            self.skip_trivia();
            if !self.at(SK::Comma) { break; }
            self.bump();
            self.skip_trivia();
            if self.at(SK::Name) { self.bump(); }
            else { self.error_here("expected name after `,`".to_string()); break; }
        }
        self.builder.finish_node();
    }

    fn parse_param_list(&mut self) {
        self.skip_trivia();
        self.builder.start_node(SK::ParameterList);
        if !self.bump_if(SK::LeftBracket) {
            self.error_here("expected `(`".to_string());
            self.builder.finish_node();
            return;
        }
        self.skip_trivia();
        if !self.at(SK::RightBracket) {
            loop {
                self.skip_trivia();
                if self.at(SK::TripleDot) {
                    let vt = self.peek().unwrap();
                    self.builder.token(SK::ParameterVarArgs, vt.start, vt.end);
                    self.advance();
                    break;
                } else if self.at(SK::Name) || self.peek().is_some_and(|t| t.kind.is_keyword()) {
                    // Accept keywords as parameter names (e.g. `repeat` in DoTradeSkill)
                    let tok = self.peek().unwrap();
                    self.builder.token(SK::Parameter, tok.start, tok.end);
                    self.advance();
                } else {
                    break;
                }
                self.skip_trivia();
                if !self.at(SK::Comma) { break; }
                self.bump();
            }
        }
        self.skip_trivia();
        if !self.bump_if(SK::RightBracket) {
            self.error_here("expected `)`".to_string());
        }
        self.builder.finish_node();
    }

    // ── Table constructor ──

    fn parse_table_constructor(&mut self) {
        self.builder.start_node(SK::TableConstructor);
        self.bump(); // `{`
        self.skip_trivia();
        if !self.at(SK::RightCurlyBracket) {
            self.parse_field_list();
        }
        self.skip_trivia();
        if !self.bump_if(SK::RightCurlyBracket) {
            self.error_here("expected `}`".to_string());
        }
        self.builder.finish_node();
    }

    fn parse_field_list(&mut self) {
        loop {
            self.skip_trivia();
            if self.at(SK::RightCurlyBracket) { break; }

            self.builder.start_node(SK::Field);
            let Some(tok) = self.peek() else { self.builder.finish_node(); break };

            if tok.kind == SK::LeftSquareBracket {
                // [expr] = expr
                self.bump();
                self.skip_trivia();
                self.parse_expression();
                self.skip_trivia();
                if !self.bump_if(SK::RightSquareBracket) { self.error_here("expected `]`".to_string()); }
                self.skip_trivia();
                if !self.bump_if(SK::Assign) { self.error_here("expected `=`".to_string()); }
                self.skip_trivia();
                self.parse_expression();
            } else if tok.kind == SK::Name {
                // Could be `name = expr` or a positional value expression.
                // Peek ahead: consume name, check for `=`.
                // We need to handle this carefully since the expression parser
                // will consume the name as a NameRef.
                let name_tok = self.advance().unwrap();
                let mut trivia_after = Vec::new();
                while let Some(t) = self.peek() {
                    if t.kind.is_trivia() { trivia_after.push(self.advance().unwrap()); } else { break; }
                }
                if self.peek().is_some_and(|t| t.kind == SK::Assign) {
                    // name = expr
                    self.builder.token(SK::Name, name_tok.start, name_tok.end);
                    self.emit_trivia(&trivia_after);
                    self.bump(); // `=`
                    self.skip_trivia();
                    self.parse_expression();
                } else {
                    // Positional expression starting with a name.
                    // Re-inject the name and trivia and parse as expression.
                    self.builder.start_node(SK::NameRef);
                    self.builder.token(SK::Name, name_tok.start, name_tok.end);
                    self.builder.finish_node();
                    self.emit_trivia(&trivia_after);
                    // Parse any suffixes (e.g. `foo.bar` or `foo()` as positional)
                    self.parse_suffixes();
                    // Continue with any binary operators
                    self.parse_expr_bp_continue(0);
                }
            } else if !self.parse_expression() {
                self.builder.finish_node();
                break;
            }
            self.builder.finish_node();

            self.skip_trivia();
            if self.at(SK::Comma) || self.at(SK::Semicolon) { self.bump(); }
            else { break; }
        }
    }

    /// Continue Pratt parsing from an already-parsed left operand.
    /// Used when we've already emitted the base expression and need to check for operators.
    fn parse_expr_bp_continue(&mut self, min_bp: u8) {
        let cp = self.checkpoint_before_last();
        loop {
            self.skip_trivia();
            let Some(tok) = self.peek() else { break };
            let Some((left_bp, right_bp)) = infix_binding_power(tok.kind) else { break };
            if left_bp < min_bp { break; }
            self.builder.start_node_at(cp, SK::BinaryExpression);
            self.bump();
            self.skip_trivia();
            if !self.parse_expr_bp(right_bp) {
                self.error_here("expected expression after operator".to_string());
            }
            self.builder.finish_node();
        }
    }

    // ── Helpers ──

    fn emit_trivia(&mut self, trivia: &[Tok]) {
        for t in trivia {
            self.builder.token(t.kind, t.start, t.end);
        }
    }
}

// ── Operator precedence ──

const UNARY_BP: u8 = 12;

fn infix_binding_power(kind: SK) -> Option<(u8, u8)> {
    match kind {
        SK::OrKeyword => Some((1, 2)),
        SK::AndKeyword => Some((3, 4)),
        SK::LessThan | SK::GreaterThan | SK::LessThanOrEquals | SK::GreaterThanOrEquals
            | SK::EqualsBoolean | SK::NotEqualsBoolean => Some((5, 6)),
        SK::DoubleDot => Some((8, 7)),
        SK::Plus | SK::Minus => Some((9, 10)),
        SK::Asterisk | SK::Slash | SK::Modulo => Some((11, 12)),
        SK::Hat => Some((15, 14)),
        _ => None,
    }
}

/// Parse a Lua source string into a SyntaxTree.
pub fn parse(text: &str) -> SyntaxTree {
    Parser::new(text).parse()
}

/// Returns true for keywords that can terminate a block (end / else / elseif / until).
/// Used by parse_block to break early on any outer-block terminator, preventing
/// cascade failures when a nested block is missing its own terminator.
fn is_block_terminator(kind: SK) -> bool {
    matches!(kind, SK::EndKeyword | SK::ElseKeyword | SK::ElseIfKeyword | SK::UntilKeyword)
}

/// Returns true for token kinds that can legally start a new statement.
/// Used by parse_statement's skip-ahead recovery to find the next safe parse point.
fn is_stmt_starter(kind: SK) -> bool {
    matches!(
        kind,
        SK::DoKeyword
            | SK::WhileKeyword
            | SK::RepeatKeyword
            | SK::IfKeyword
            | SK::ForKeyword
            | SK::LocalKeyword
            | SK::FunctionKeyword
            | SK::ReturnKeyword
            | SK::BreakKeyword
            | SK::Name
            | SK::LeftBracket
            | SK::Semicolon
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::tree::{Child, SyntaxTree, NodeId};

    fn dump_tree(tree: &SyntaxTree) -> String {
        let mut out = String::new();
        dump_node(tree, tree.root(), 0, &mut out);
        out
    }

    fn dump_node(tree: &SyntaxTree, id: NodeId, indent: usize, out: &mut String) {
        let node = tree.node(id);
        let prefix = "  ".repeat(indent);
        out.push_str(&format!("{}{:?} {}..{}\n", prefix, node.kind, node.start, node.end));
        for child in tree.node_children(id) {
            match child {
                Child::Node(nid) => dump_node(tree, *nid, indent + 1, out),
                Child::Token(tid) => {
                    let tok = tree.token(*tid);
                    let text = tree.token_text(*tid);
                    let tprefix = "  ".repeat(indent + 1);
                    out.push_str(&format!("{}{:?} {:?} {}..{}\n", tprefix, tok.kind, text, tok.start, tok.end));
                }
            }
        }
    }

    #[test]
    fn test_simple_local() {
        let tree = parse("local x = 5");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("LocalAssignStatement"), "tree:\n{}", dump);
        assert!(dump.contains("\"x\""), "tree:\n{}", dump);
        assert!(dump.contains("\"5\""), "tree:\n{}", dump);
    }

    #[test]
    fn test_function_def() {
        let tree = parse("function foo(x, y) return x + y end");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("FunctionDefinition"), "tree:\n{}", dump);
        assert!(dump.contains("ReturnStatement"), "tree:\n{}", dump);
        assert!(dump.contains("BinaryExpression"), "tree:\n{}", dump);
    }

    #[test]
    fn test_if_chain() {
        let tree = parse("if x then y() elseif z then w() else q() end");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("IfChain"), "tree:\n{}", dump);
        assert!(dump.contains("IfBranch"), "tree:\n{}", dump);
        assert!(dump.contains("ElseBranch"), "tree:\n{}", dump);
    }

    #[test]
    fn test_method_call() {
        let tree = parse("x:foo(1, 2)");
        let dump = dump_tree(&tree);
        assert!(dump.contains("MethodCall"), "tree:\n{}", dump);
        assert!(dump.contains("NameRef"), "tree:\n{}", dump);
    }

    #[test]
    fn test_dot_access() {
        let tree = parse("local a = x.y.z");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        // x.y.z should be DotAccess(DotAccess(NameRef("x"), "y"), "z")
        assert!(dump.contains("DotAccess"), "tree:\n{}", dump);
        assert!(dump.contains("NameRef"), "tree:\n{}", dump);
    }

    #[test]
    fn test_bracket_access() {
        let tree = parse("local a = t[1]");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("BracketAccess"), "tree:\n{}", dump);
    }

    #[test]
    fn test_chained_access() {
        // a.b[1]:c(x)
        let tree = parse("a.b[1]:c(x)");
        let dump = dump_tree(&tree);
        assert!(dump.contains("DotAccess"), "tree:\n{}", dump);
        assert!(dump.contains("BracketAccess"), "tree:\n{}", dump);
        assert!(dump.contains("MethodCall"), "tree:\n{}", dump);
    }

    #[test]
    fn test_for_count() {
        let tree = parse("for i = 1, 10 do print(i) end");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("ForCountLoop"), "tree:\n{}", dump);
    }

    #[test]
    fn test_for_in() {
        let tree = parse("for k, v in pairs(t) do print(k) end");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("ForInLoop"), "tree:\n{}", dump);
    }

    #[test]
    fn test_table_constructor() {
        let tree = parse("local t = { a = 1, b = 2, 3 }");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("TableConstructor"), "tree:\n{}", dump);
        assert!(dump.contains("Field"), "tree:\n{}", dump);
    }

    #[test]
    fn test_assignment() {
        let tree = parse("x = 5");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("AssignStatement"), "tree:\n{}", dump);
        assert!(dump.contains("VariableList"), "tree:\n{}", dump);
    }

    #[test]
    fn test_multi_assignment() {
        let tree = parse("a, b = 1, 2");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("AssignStatement"), "tree:\n{}", dump);
    }

    #[test]
    fn test_binary_precedence() {
        // 1 + 2 * 3 should parse as 1 + (2 * 3)
        let tree = parse("local x = 1 + 2 * 3");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        // Should have nested BinaryExpression
        let binary_count = dump.matches("BinaryExpression").count();
        assert!(binary_count >= 2, "expected nested BinaryExpression, tree:\n{}", dump);
    }

    #[test]
    fn test_unary_expr() {
        let tree = parse("local x = -5");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("UnaryExpression"), "tree:\n{}", dump);
    }

    #[test]
    fn test_grouped_expr() {
        let tree = parse("local x = (1 + 2)");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("GroupedExpression"), "tree:\n{}", dump);
    }

    #[test]
    fn test_funcall_access() {
        // func().field
        let tree = parse("local x = foo().bar");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("FunctionCall"), "tree:\n{}", dump);
        assert!(dump.contains("DotAccess"), "tree:\n{}", dump);
    }

    #[test]
    fn test_grouped_access() {
        // (expr).field
        let tree = parse("local x = (t).bar");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("GroupedExpression"), "tree:\n{}", dump);
        assert!(dump.contains("DotAccess"), "tree:\n{}", dump);
    }

    #[test]
    fn test_token_at_offset() {
        let tree = parse("local x = 5");
        match tree.token_at_offset(6) {
            crate::syntax::tree::TokenAtOffset::Single(tid) => {
                assert_eq!(tree.token_text(tid), "x");
            }
            other => panic!("expected Single, got {:?}", other),
        }
    }

    #[test]
    fn test_prev_next_token() {
        let tree = parse("local x");
        let last = crate::syntax::tree::TokenId(tree.token_count() - 1);
        let mut tid = Some(last);
        let mut texts = Vec::new();
        while let Some(id) = tid {
            texts.push(tree.token_text(id).to_string());
            tid = tree.prev_token(id);
        }
        texts.reverse();
        assert_eq!(texts, vec!["local", " ", "x"]);
    }

    #[test]
    fn test_repeat_loop() {
        let tree = parse("repeat x = x + 1 until x > 10");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("RepeatUntilLoop"), "tree:\n{}", dump);
    }

    #[test]
    fn test_do_block() {
        let tree = parse("do local x = 1 end");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("DoBlock"), "tree:\n{}", dump);
    }

    #[test]
    fn test_while_loop() {
        let tree = parse("while true do break end");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("WhileLoop"), "tree:\n{}", dump);
    }

    #[test]
    fn test_function_expression() {
        let tree = parse("local f = function(x) return x end");
        assert!(tree.errors.is_empty(), "errors: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("FunctionDefinition"), "tree:\n{}", dump);
    }

    #[test]
    fn test_string_call() {
        // foo "hello" is a valid call
        let tree = parse("print \"hello\"");
        let dump = dump_tree(&tree);
        assert!(dump.contains("FunctionCall"), "tree:\n{}", dump);
    }

    #[test]
    fn test_table_call() {
        // foo{1,2} is a valid call
        let tree = parse("print{1, 2}");
        let dump = dump_tree(&tree);
        assert!(dump.contains("FunctionCall"), "tree:\n{}", dump);
    }

    #[test]
    fn test_parse_all_test_lua_files() {
        // Stress test: parse every .lua file in tests/ through the new parser.
        // This should never panic.
        let test_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let mut count = 0;
        for entry in walkdir(&test_dir) {
            let text = std::fs::read_to_string(&entry).unwrap();
            let tree = parse(&text);
            // Just verify it parsed without panicking and produced a root node.
            let _ = tree.node(tree.root());
            count += 1;
        }
        assert!(count > 20, "expected to parse >20 files, got {}", count);
    }

    #[test]
    fn test_lossless_token_coverage() {
        // Verify every byte in source is covered by exactly one token (no gaps/overlaps).
        let test_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let mut count = 0;
        for entry in walkdir(&test_dir) {
            let text = std::fs::read_to_string(&entry).unwrap();
            if text.is_empty() { continue; }
            let tree = parse(&text);
            let mut expected = 0u32;
            for i in 0..tree.token_count() {
                let tid = crate::syntax::tree::TokenId(i);
                let tok = tree.token(tid);
                assert_eq!(tok.start, expected,
                    "gap/overlap at {} in {:?}: token {:?} starts at {}",
                    expected, entry.file_name().unwrap(), tok.kind, tok.start);
                assert!(tok.end > tok.start);
                expected = tok.end;
            }
            assert_eq!(expected, text.len() as u32,
                "tokens don't cover source in {:?}", entry.file_name().unwrap());
            count += 1;
        }
        assert!(count > 20);
    }

    // ── Error-recovery tests ──────────────────────────────────────────────────

    /// Stray tokens at statement level should be skipped in one pass so that the
    /// statement AFTER them is still parsed correctly.
    #[test]
    fn test_spurious_tokens_recovery() {
        // Numbers / operators are not valid statements; the parser should skip them
        // and still produce a well-formed LocalAssignStatement for `local y = 10`.
        let tree = parse("local x = 5\n42 + 7\nlocal y = 10");
        let dump = dump_tree(&tree);
        assert!(!tree.errors.is_empty(), "expected parse errors");
        // Both locals must be present in the AST
        assert_eq!(dump.matches("LocalAssignStatement").count(), 2, "tree:\n{}", dump);
    }

    /// A stray `end` at the top level should be consumed as a single error, leaving
    /// subsequent code intact.
    #[test]
    fn test_stray_end_at_top_level() {
        let tree = parse("local x = 1\nend\nlocal y = 2");
        let dump = dump_tree(&tree);
        assert!(!tree.errors.is_empty(), "expected parse error for stray `end`");
        assert_eq!(dump.matches("LocalAssignStatement").count(), 2, "tree:\n{}", dump);
    }

    /// A stray `until` inside a `while` body (not a repeat body) should not be
    /// parsed as a statement — the while block should close early and the outer
    /// context should see the stray token.
    #[test]
    fn test_stray_until_in_while_body() {
        // The `until` should cause the while body to close, while the `end` that
        // follows closes the while loop itself (with an error for missing its end
        // after the until escape).
        let tree = parse("while true do\n  local x = 1\n  until\nend");
        // We expect errors (stray `until`, mismatched terminator)
        assert!(!tree.errors.is_empty(), "expected parse errors");
        // But the tree must still have a WhileLoop node — the parse must not panic
        let dump = dump_tree(&tree);
        assert!(dump.contains("WhileLoop"), "tree:\n{}", dump);
    }

    /// A missing `end` for a function should emit an error but leave subsequent
    /// top-level code parseable.
    #[test]
    fn test_missing_end_recovery() {
        let tree = parse("function foo()\n  local x = 1\n\nlocal z = 99");
        let dump = dump_tree(&tree);
        assert!(!tree.errors.is_empty(), "expected error for missing `end`");
        // foo's FunctionDefinition must appear
        assert!(dump.contains("FunctionDefinition"), "tree:\n{}", dump);
        // `z` should still be present somewhere in the tree
        assert!(dump.contains("\"z\""), "tree:\n{}", dump);
    }

    /// An incomplete expression (unclosed parenthesis) should be localised so that
    /// the statement on the next line still parses cleanly.
    ///
    /// Recovery path: `parse_call_args` sees `local` (not a valid expression primary)
    /// when trying to parse arguments, so `parse_expression_list` returns false, no args
    /// are consumed, `bump_if(RightBracket)` fails → "expected `)` " error, and control
    /// returns to the block loop which then sees `local y` and parses it normally.
    /// This works without any changes to the expression parser — it is a regression
    /// guard for pre-existing behaviour.
    #[test]
    fn test_unclosed_paren_recovery() {
        let tree = parse("local x = foo(\nlocal y = 10");
        let dump = dump_tree(&tree);
        // The unclosed call generates an error
        assert!(!tree.errors.is_empty(), "expected error for unclosed `(`");
        // But `local y` must still appear in the tree
        assert_eq!(dump.matches("LocalAssignStatement").count(), 2, "tree:\n{}", dump);
    }

    /// Multiple consecutive bad tokens should produce a single error span, not one
    /// error per token.
    #[test]
    fn test_multiple_bad_tokens_single_error() {
        // Three non-statement tokens before a valid statement
        let tree = parse("+ - *\nlocal x = 1");
        assert!(!tree.errors.is_empty(), "expected parse errors");
        // Only one error should cover the whole run of bad tokens
        assert_eq!(tree.errors.len(), 1, "expected single error, got: {:?}", tree.errors);
        let dump = dump_tree(&tree);
        assert!(dump.contains("LocalAssignStatement"), "tree:\n{}", dump);
    }

    fn walkdir(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    files.extend(walkdir(&path));
                } else if path.extension().map_or(false, |e| e == "lua") {
                    files.push(path);
                }
            }
        }
        files
    }
}

