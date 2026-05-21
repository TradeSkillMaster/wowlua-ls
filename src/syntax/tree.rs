use crate::syntax::SyntaxKind;

/// Index into the node arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NodeId(pub u32);

/// Index into the token arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenId(pub u32);

/// A token in the source. Stored in a flat Vec sorted by byte offset.
#[derive(Debug, Clone, Copy)]
pub struct Token {
    pub kind: SyntaxKind,
    pub start: u32,
    pub end: u32,
    /// Which node directly owns this token as a child.
    pub(crate) parent_node: NodeId,
}

/// A composite node in the syntax tree.
#[derive(Debug, Clone)]
pub struct Node {
    pub kind: SyntaxKind,
    /// Byte offset of the first non-trivia content.
    pub start: u32,
    /// Byte offset of the last non-trivia content end (no trailing trivia).
    pub end: u32,
    pub(crate) parent: Option<NodeId>,
    /// Range into the children array: children[children_start..children_start+children_count]
    pub(crate) children_start: u32,
    /// Number of direct children.
    pub(crate) children_count: u32,
}

/// A child entry can be either a sub-node or a token.
#[derive(Debug, Clone, Copy)]
pub enum Child {
    Node(NodeId),
    Token(TokenId),
}

/// A parse error.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub start: u32,
    pub end: u32,
    pub message: String,
}

/// Result of looking up a token at a byte offset.
#[derive(Debug)]
pub enum TokenAtOffset<T> {
    /// No token found (offset out of range).
    None,
    /// Offset falls within exactly one token.
    Single(T),
    /// Offset falls between two adjacent tokens (at a boundary).
    Between(T, T),
}

impl<T> TokenAtOffset<T> {
    pub(crate) fn right_biased(self) -> Option<T> {
        match self {
            Self::None => None,
            Self::Single(t) => Some(t),
            Self::Between(_, right) => Some(right),
        }
    }
    pub(crate) fn left_biased(self) -> Option<T> {
        match self {
            Self::None => None,
            Self::Single(t) => Some(t),
            Self::Between(left, _) => Some(left),
        }
    }
}

/// A saved position in the current node's child list.
/// Used with `TreeBuilder::start_node_at()` for retroactive wrapping.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Checkpoint(u32);

/// The complete parsed syntax tree.
pub struct SyntaxTree {
    source: String,
    pub(crate) nodes: Vec<Node>,
    pub(crate) tokens: Vec<Token>,
    pub(crate) children: Vec<Child>,
    pub errors: Vec<ParseError>,
}

impl SyntaxTree {
    /// Get the original source text.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Get the root node (always index 0).
    pub fn root(&self) -> NodeId {
        NodeId(0)
    }

    // ── Node access ──

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    pub fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.nodes[id.0 as usize].kind
    }

    pub(crate) fn node_parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id.0 as usize].parent
    }

    /// Iterate the direct children of a node.
    pub fn node_children(&self, id: NodeId) -> &[Child] {
        let node = &self.nodes[id.0 as usize];
        let start = node.children_start as usize;
        let end = start + node.children_count as usize;
        &self.children[start..end]
    }

    /// Iterate only child nodes (not tokens) of a node.
    pub(crate) fn child_nodes(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.node_children(id).iter().filter_map(|c| match c {
            Child::Node(nid) => Some(*nid),
            Child::Token(_) => None,
        })
    }

    // ── Token access ──

    /// Iterate all tokens in source order.
    pub fn all_tokens(&self) -> &[Token] {
        &self.tokens
    }

    pub fn token(&self, id: TokenId) -> &Token {
        &self.tokens[id.0 as usize]
    }

    pub(crate) fn token_kind(&self, id: TokenId) -> SyntaxKind {
        self.tokens[id.0 as usize].kind
    }

    pub fn token_text(&self, id: TokenId) -> &str {
        let t = &self.tokens[id.0 as usize];
        &self.source[t.start as usize..t.end as usize]
    }

    pub(crate) fn token_parent(&self, id: TokenId) -> NodeId {
        self.tokens[id.0 as usize].parent_node
    }

    #[cfg(test)]
    pub(crate) fn token_count(&self) -> u32 {
        self.tokens.len() as u32
    }

    // ── Token navigation (O(1) since tokens are source-ordered) ──

    pub(crate) fn prev_token(&self, id: TokenId) -> Option<TokenId> {
        if id.0 == 0 { None } else { Some(TokenId(id.0 - 1)) }
    }

    pub(crate) fn next_token(&self, id: TokenId) -> Option<TokenId> {
        let next = id.0 + 1;
        if (next as usize) < self.tokens.len() { Some(TokenId(next)) } else { None }
    }

    // ── Position queries (O(log n) via binary search) ──

    /// Find the token at a given byte offset.
    pub(crate) fn token_at_offset(&self, offset: u32) -> TokenAtOffset<TokenId> {
        if self.tokens.is_empty() {
            return TokenAtOffset::None;
        }

        // Binary search: find the last token whose start <= offset
        let idx = self.tokens.partition_point(|t| t.start <= offset);

        if idx == 0 {
            // offset is before all tokens
            if !self.tokens.is_empty() && self.tokens[0].start == offset {
                return TokenAtOffset::Single(TokenId(0));
            }
            return TokenAtOffset::None;
        }

        let left_idx = idx - 1;
        let left = &self.tokens[left_idx];

        // Check if offset is within the left token
        if offset < left.end {
            return TokenAtOffset::Single(TokenId(left_idx as u32));
        }

        // Offset is at or past the end of left token
        if idx < self.tokens.len() {
            let right = &self.tokens[idx];
            if offset == left.end && offset == right.start {
                // Exactly between two tokens
                return TokenAtOffset::Between(TokenId(left_idx as u32), TokenId(idx as u32));
            }
            if offset < right.start {
                // Gap between tokens — shouldn't happen in a lossless tree.
                debug_assert!(false, "gap at offset {} between tokens {}..{} and {}..{}", offset, left.start, left.end, right.start, right.end);
                return TokenAtOffset::Between(TokenId(left_idx as u32), TokenId(idx as u32));
            }
            if offset < right.end {
                return TokenAtOffset::Single(TokenId(idx as u32));
            }
        }

        TokenAtOffset::None
    }

    // ── Descendant iteration ──

    /// Iterate all descendant nodes in pre-order (depth-first).
    pub(crate) fn descendants(&self, id: NodeId) -> DescendantNodes<'_> {
        DescendantNodes { tree: self, stack: vec![id] }
    }

    /// Iterate all descendant nodes and tokens in pre-order.
    /// Nodes come before their children.
    pub(crate) fn descendants_with_tokens(&self, id: NodeId) -> DescendantAll<'_> {
        DescendantAll { tree: self, stack: vec![DescItem::Node(id)] }
    }

}

// ── Descendant iterators ──

/// Iterator over descendant nodes in pre-order (depth-first).
pub(crate) struct DescendantNodes<'a> {
    tree: &'a SyntaxTree,
    stack: Vec<NodeId>,
}

impl<'a> Iterator for DescendantNodes<'a> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let nid = self.stack.pop()?;
        // Push children in reverse so first child is visited first
        let children = self.tree.node_children(nid);
        for child in children.iter().rev() {
            if let Child::Node(child_nid) = child {
                self.stack.push(*child_nid);
            }
        }
        Some(nid)
    }
}

enum DescItem {
    Node(NodeId),
    Token(TokenId),
}

/// Iterator over descendant nodes and tokens in pre-order.
pub(crate) struct DescendantAll<'a> {
    tree: &'a SyntaxTree,
    stack: Vec<DescItem>,
}

impl<'a> Iterator for DescendantAll<'a> {
    type Item = Child;
    fn next(&mut self) -> Option<Child> {
        let item = self.stack.pop()?;
        match item {
            DescItem::Token(tid) => Some(Child::Token(tid)),
            DescItem::Node(nid) => {
                let children = self.tree.node_children(nid);
                for child in children.iter().rev() {
                    match child {
                        Child::Node(child_nid) => self.stack.push(DescItem::Node(*child_nid)),
                        Child::Token(child_tid) => self.stack.push(DescItem::Token(*child_tid)),
                    }
                }
                Some(Child::Node(nid))
            }
        }
    }
}

// ── Tree builder ──

/// Incrementally builds a SyntaxTree during parsing.
pub(crate) struct TreeBuilder {
    source: String,
    nodes: Vec<Node>,
    tokens: Vec<Token>,
    children: Vec<Child>,
    errors: Vec<ParseError>,
    /// Stack of (NodeId, Vec<Child>, already_registered) for nodes currently being built.
    /// When we finish a node, we flush its children into the flat children array.
    /// The `already_registered` flag is true for nodes created via `start_node_at`,
    /// which pre-registers the node as a child of its parent.
    node_stack: Vec<(NodeId, Vec<Child>, bool)>,
}

impl TreeBuilder {
    pub(crate) fn new(source: String) -> Self {
        Self {
            source,
            nodes: Vec::new(),
            tokens: Vec::new(),
            children: Vec::new(),
            errors: Vec::new(),
            node_stack: Vec::new(),
        }
    }

    /// Start a new node. Must be paired with `finish_node()`.
    pub(crate) fn start_node(&mut self, kind: SyntaxKind) {
        let id = NodeId(self.nodes.len() as u32);
        let parent = self.node_stack.last().map(|(pid, _, _)| *pid);
        self.nodes.push(Node {
            kind,
            start: u32::MAX, // will be set when first child is added
            end: 0,
            parent,
            children_start: 0,
            children_count: 0,
        });
        self.node_stack.push((id, Vec::new(), false));
    }

    /// Finish the current node, flushing its children into the flat array.
    pub(crate) fn finish_node(&mut self) {
        let (id, local_children, already_registered) = self.node_stack.pop().expect("finish_node without matching start_node");
        let children_start = self.children.len() as u32;
        let children_count = local_children.len() as u32;

        // Compute node start/end from children
        let mut start = u32::MAX;
        let mut end = 0u32;
        for child in &local_children {
            match child {
                Child::Node(nid) => {
                    let n = &self.nodes[nid.0 as usize];
                    if n.start < start { start = n.start; }
                    if n.end > end { end = n.end; }
                }
                Child::Token(tid) => {
                    let t = &self.tokens[tid.0 as usize];
                    if !t.kind.is_trivia() {
                        if t.start < start { start = t.start; }
                        if t.end > end { end = t.end; }
                    }
                }
            }
        }

        self.children.extend_from_slice(&local_children);

        let node = &mut self.nodes[id.0 as usize];
        node.children_start = children_start;
        node.children_count = children_count;
        if start != u32::MAX {
            node.start = start;
        }
        node.end = end;

        // Register this node as a child of its parent (skip if start_node_at already did this)
        if !already_registered
            && let Some((_, parent_children, _)) = self.node_stack.last_mut() {
                parent_children.push(Child::Node(id));
            }
    }

    /// Add a token to the current node.
    pub(crate) fn token(&mut self, kind: SyntaxKind, start: u32, end: u32) {
        let parent_node = self.node_stack.last()
            .map(|(pid, _, _)| *pid)
            .unwrap_or(NodeId(0));
        let tid = TokenId(self.tokens.len() as u32);
        self.tokens.push(Token {
            kind,
            start,
            end,
            parent_node,
        });
        if let Some((_, children, _)) = self.node_stack.last_mut() {
            children.push(Child::Token(tid));
        }
    }

    // ── Checkpoint mechanism (retroactive wrapping) ──

    /// Save a checkpoint at the current position in the parent's child list.
    /// Later, `start_node_at(checkpoint, kind)` wraps all children added since
    /// this checkpoint in a new node of the given kind.
    pub(crate) fn checkpoint(&self) -> Checkpoint {
        let children_len = self.node_stack.last().map_or(0, |(_, c, _)| c.len());
        Checkpoint(children_len as u32)
    }

    /// Save a checkpoint positioned just before the last child of the current node.
    /// This allows wrapping the most recently emitted child(ren) in a new node.
    pub(crate) fn checkpoint_before_last(&self) -> Checkpoint {
        let children_len = self.node_stack.last().map_or(0, |(_, c, _)| c.len());
        Checkpoint(children_len.saturating_sub(1) as u32)
    }

    /// Retroactively wrap all children added since `cp` in a new node of `kind`.
    /// This is equivalent to rowan's `start_node_at(checkpoint)`.
    /// Must be paired with a subsequent `finish_node()`.
    pub(crate) fn start_node_at(&mut self, cp: Checkpoint, kind: SyntaxKind) {
        let new_id = NodeId(self.nodes.len() as u32);
        let parent = self.node_stack.last().map(|(pid, _, _)| *pid);

        self.nodes.push(Node {
            kind,
            start: u32::MAX,
            end: 0,
            parent,
            children_start: 0,
            children_count: 0,
        });

        // Take all children from cp.0.. out of the parent's child list
        let (_, parent_children, _) = self.node_stack.last_mut().expect("start_node_at without open node");
        let wrapped: Vec<Child> = parent_children.drain(cp.0 as usize..).collect();

        // Update parent pointers for wrapped children
        for child in &wrapped {
            match child {
                Child::Node(nid) => {
                    self.nodes[nid.0 as usize].parent = Some(new_id);
                }
                Child::Token(tid) => {
                    self.tokens[tid.0 as usize].parent_node = new_id;
                }
            }
        }

        // Add the new wrapping node as a child of the current parent
        parent_children.push(Child::Node(new_id));

        // Push the new node with its wrapped children onto the stack
        self.node_stack.push((new_id, wrapped, true));
    }

    /// Record a parse error.
    pub(crate) fn error(&mut self, start: u32, end: u32, message: String) {
        self.errors.push(ParseError { start, end, message });
    }

    /// Finalize and return the completed tree.
    pub(crate) fn finish(self) -> SyntaxTree {
        debug_assert!(self.node_stack.is_empty(), "unfinished nodes on stack");
        SyntaxTree {
            source: self.source,
            nodes: self.nodes,
            tokens: self.tokens,
            children: self.children,
            errors: self.errors,
        }
    }

}

// ── High-level syntax API ──
// Wraps arena-based SyntaxTree in ergonomic node/token types with
// method-based navigation (.kind(), .children(), .parent(), .text(),
// .token_at_offset(), etc.).


// ── TextSize / TextRange (drop-in replacements for rowan types) ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextSize(pub u32);

impl From<u32> for TextSize {
    fn from(v: u32) -> Self { Self(v) }
}
impl From<TextSize> for u32 {
    fn from(v: TextSize) -> Self { v.0 }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextRange {
    start: TextSize,
    end: TextSize,
}

impl TextRange {
    pub(crate) fn new(start: TextSize, end: TextSize) -> Self {
        Self { start, end }
    }
    pub fn start(&self) -> TextSize { self.start }
    pub fn end(&self) -> TextSize { self.end }
}

// ── NodeOrToken ──

pub enum NodeOrToken<N, T> {
    Node(N),
    Token(T),
}

impl<N, T> NodeOrToken<N, T> {
    pub(crate) fn into_token(self) -> Option<T> {
        match self { Self::Token(t) => Some(t), _ => None }
    }
    pub(crate) fn as_token(&self) -> Option<&T> {
        match self { Self::Token(t) => Some(t), _ => None }
    }
}

// ── SyntaxNode ──

#[derive(Clone, Copy)]
pub struct SyntaxNode<'a> {
    pub(crate) tree: &'a SyntaxTree,
    pub(crate) id: NodeId,
}

impl std::fmt::Debug for SyntaxNode<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SyntaxNode({:?}, {:?}..{:?})", self.kind(), self.text_range().start(), self.text_range().end())
    }
}

impl<'a> SyntaxNode<'a> {
    pub fn new_root(tree: &'a SyntaxTree) -> Self {
        Self { tree, id: tree.root() }
    }

    pub(crate) fn kind(&self) -> SyntaxKind {
        self.tree.node_kind(self.id)
    }

    pub(crate) fn text_range(&self) -> TextRange {
        let node = self.tree.node(self.id);
        if node.start == u32::MAX {
            return TextRange::new(TextSize(0), TextSize(0));
        }
        TextRange::new(TextSize(node.start), TextSize(node.end))
    }

    pub(crate) fn parent(&self) -> Option<SyntaxNode<'a>> {
        self.tree.node_parent(self.id).map(|id| SyntaxNode { tree: self.tree, id })
    }

    pub(crate) fn children(&self) -> impl Iterator<Item = SyntaxNode<'a>> + '_ {
        self.tree.child_nodes(self.id)
            .map(|id| SyntaxNode { tree: self.tree, id })
    }

    pub(crate) fn children_with_tokens(&self) -> impl Iterator<Item = NodeOrToken<SyntaxNode<'a>, SyntaxToken<'a>>> + '_ {
        self.tree.node_children(self.id).iter().map(|child| {
            match child {
                Child::Node(nid) => NodeOrToken::Node(SyntaxNode { tree: self.tree, id: *nid }),
                Child::Token(tid) => NodeOrToken::Token(SyntaxToken { tree: self.tree, id: *tid }),
            }
        })
    }

    pub(crate) fn first_token(&self) -> Option<SyntaxToken<'a>> {
        self.find_first_token(self.id)
    }

    fn find_first_token(&self, id: NodeId) -> Option<SyntaxToken<'a>> {
        for child in self.tree.node_children(id) {
            match child {
                Child::Token(tid) => return Some(SyntaxToken { tree: self.tree, id: *tid }),
                Child::Node(nid) => {
                    if let Some(tok) = self.find_first_token(*nid) {
                        return Some(tok);
                    }
                }
            }
        }
        None
    }

    pub(crate) fn last_token(&self) -> Option<SyntaxToken<'a>> {
        self.find_last_token(self.id)
    }

    fn find_last_token(&self, id: NodeId) -> Option<SyntaxToken<'a>> {
        for child in self.tree.node_children(id).iter().rev() {
            match child {
                Child::Token(tid) => return Some(SyntaxToken { tree: self.tree, id: *tid }),
                Child::Node(nid) => {
                    if let Some(tok) = self.find_last_token(*nid) {
                        return Some(tok);
                    }
                }
            }
        }
        None
    }

    pub(crate) fn ancestors(&self) -> impl Iterator<Item = SyntaxNode<'a>> {
        let tree = self.tree;
        let mut current = Some(self.id);
        std::iter::from_fn(move || {
            let id = current?;
            let node = SyntaxNode { tree, id };
            current = tree.node_parent(id);
            Some(node)
        })
    }

    pub(crate) fn descendants(&self) -> impl Iterator<Item = SyntaxNode<'a>> + '_ {
        self.tree.descendants(self.id)
            .map(|id| SyntaxNode { tree: self.tree, id })
    }

    pub(crate) fn descendants_with_tokens(&self) -> impl Iterator<Item = NodeOrToken<SyntaxNode<'a>, SyntaxToken<'a>>> + '_ {
        self.tree.descendants_with_tokens(self.id).map(|child| match child {
            Child::Node(nid) => NodeOrToken::Node(SyntaxNode { tree: self.tree, id: nid }),
            Child::Token(tid) => NodeOrToken::Token(SyntaxToken { tree: self.tree, id: tid }),
        })
    }

    pub(crate) fn token_at_offset(&self, offset: TextSize) -> TokenAtOffset<SyntaxToken<'a>> {
        match self.tree.token_at_offset(offset.0) {
            TokenAtOffset::None => TokenAtOffset::None,
            TokenAtOffset::Single(tid) =>
                TokenAtOffset::Single(SyntaxToken { tree: self.tree, id: tid }),
            TokenAtOffset::Between(l, r) =>
                TokenAtOffset::Between(
                    SyntaxToken { tree: self.tree, id: l },
                    SyntaxToken { tree: self.tree, id: r },
                ),
        }
    }

    pub(crate) fn text(&self) -> SyntaxText<'a> {
        let node = self.tree.node(self.id);
        if node.start == u32::MAX || node.start > node.end || node.end as usize > self.tree.source().len() {
            return SyntaxText("");
        }
        SyntaxText(&self.tree.source()[node.start as usize..node.end as usize])
    }

    /// Check first child matching predicate.
    pub(crate) fn first_child_or_token_by_kind(&self, pred: &dyn Fn(SyntaxKind) -> bool) -> Option<NodeOrToken<SyntaxNode<'a>, SyntaxToken<'a>>> {
        for child in self.tree.node_children(self.id) {
            let kind = match child {
                Child::Node(nid) => self.tree.node_kind(*nid),
                Child::Token(tid) => self.tree.token_kind(*tid),
            };
            if pred(kind) {
                return Some(match child {
                    Child::Node(nid) => NodeOrToken::Node(SyntaxNode { tree: self.tree, id: *nid }),
                    Child::Token(tid) => NodeOrToken::Token(SyntaxToken { tree: self.tree, id: *tid }),
                });
            }
        }
        None
    }
}

// ── SyntaxToken ──

#[derive(Clone, Copy)]
pub struct SyntaxToken<'a> {
    pub(crate) tree: &'a SyntaxTree,
    pub(crate) id: TokenId,
}

impl<'a> SyntaxToken<'a> {
    pub(crate) fn kind(&self) -> SyntaxKind {
        self.tree.token_kind(self.id)
    }

    pub(crate) fn text(&self) -> &'a str {
        self.tree.token_text(self.id)
    }

    pub(crate) fn text_range(&self) -> TextRange {
        let tok = self.tree.token(self.id);
        TextRange::new(TextSize(tok.start), TextSize(tok.end))
    }

    /// Returns the parent node of this token.
    /// Every token always has a parent node, so this always returns `Some`.
    /// The `Option` is retained for API compatibility with callers using `?`.
    pub(crate) fn parent(&self) -> Option<SyntaxNode<'a>> {
        let parent_id = self.tree.token_parent(self.id);
        Some(SyntaxNode { tree: self.tree, id: parent_id })
    }

    pub(crate) fn prev_token(&self) -> Option<SyntaxToken<'a>> {
        self.tree.prev_token(self.id).map(|id| SyntaxToken { tree: self.tree, id })
    }

    pub(crate) fn next_token(&self) -> Option<SyntaxToken<'a>> {
        self.tree.next_token(self.id).map(|id| SyntaxToken { tree: self.tree, id })
    }

    /// Walk ancestor nodes starting from this token's parent.
    pub(crate) fn ancestors(&self) -> impl Iterator<Item = SyntaxNode<'a>> {
        let tree = self.tree;
        let parent_id = self.tree.token_parent(self.id);
        let mut current = Some(parent_id);
        std::iter::from_fn(move || {
            let id = current?;
            let node = SyntaxNode { tree, id };
            current = tree.node_parent(id);
            Some(node)
        })
    }
}

// ── SyntaxText ──

pub(crate) struct SyntaxText<'a>(pub &'a str);

impl std::fmt::Display for SyntaxText<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

// ── NodeOrToken kind helper ──

impl<'a> NodeOrToken<SyntaxNode<'a>, SyntaxToken<'a>> {
    pub(crate) fn kind(&self) -> SyntaxKind {
        match self {
            Self::Node(n) => n.kind(),
            Self::Token(t) => t.kind(),
        }
    }
    pub(crate) fn text_range(&self) -> TextRange {
        match self {
            Self::Node(n) => n.text_range(),
            Self::Token(t) => t.text_range(),
        }
    }
}
