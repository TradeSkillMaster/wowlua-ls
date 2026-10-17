//! The names a file's function bodies declare (parameters, `local`s, loop
//! variables), scoped as the build scopes them, for the global scan of what
//! those bodies write: which names a body uses are a function's own rather than
//! the file's or globals, and which `local`s a call initializes (`local f =
//! CreateFrame(...)`), whose value is the object that call returns.

use crate::ast::{AstNode, Expression, FunctionCall, LocalAssign};
use crate::collections::HashMap;
use crate::syntax::{NodeOrToken, SyntaxKind, SyntaxNode};

/// A `local` a function body declares with one call as its only value and no
/// annotation (`local f = CreateFrame(...)`).
pub(super) struct CallLocal<'a> {
    /// The local's name, unique in the file: `name#n` for the `n`th such local
    /// named `name`. A Lua name has no `#`, so it never collides with a file
    /// local's name (`ExternalGlobal::guessed_local` / `assigned_call_local`).
    pub key: String,
    pub call: FunctionCall<'a>,
    /// Assigned again (`f = v`, `function f() end`): its value isn't only the call's.
    pub rebound: bool,
}

/// What a name used in a function body names, when a function declares it.
#[derive(Clone, Copy)]
pub(super) enum FunctionBinding {
    /// A parameter, a loop variable, or a `local` other than a [`CallLocal`].
    Other,
    /// The [`CallLocal`] at this index of [`FunctionLocals::call_locals`].
    Call(usize),
}

/// The locals of a file's function bodies (see the module docs).
pub(super) struct FunctionLocals<'a> {
    pub call_locals: Vec<CallLocal<'a>>,
    /// The binding of each name a function body uses that a function declares,
    /// by the start of its name token.
    bindings: HashMap<u32, FunctionBinding>,
}

/// Whether `key` (an `ExternalGlobal::guessed_local` or `assigned_call_local`)
/// is a [`CallLocal::key`] rather than a file local's name.
pub(crate) fn is_call_local_key(key: &str) -> bool {
    key.contains('#')
}

/// The name of the local `key` (an `ExternalGlobal::assigned_call_local`)
/// stands for: a [`CallLocal::key`]'s local, or a file local's own name.
pub(crate) fn call_local_name(key: &str) -> &str {
    key.split_once('#').map_or(key, |(name, _)| name)
}

/// A step of the walk in [`FunctionLocals::collect`].
enum Step<'a> {
    /// A node, and whether it is in a function body.
    Visit(SyntaxNode<'a>, bool),
    Declare(&'a str, FunctionBinding),
    OpenScope,
    CloseScope,
}

/// The text of `node`'s direct `kind` tokens.
fn child_tokens<'a>(node: SyntaxNode<'a>, kind: SyntaxKind) -> Vec<&'a str> {
    node.children_with_tokens().filter_map(|c| match c {
        NodeOrToken::Token(t) if t.kind() == kind => Some(t.text()),
        _ => None,
    }).collect()
}

/// The child nodes of `node` of `kind`.
fn child_nodes<'a>(node: SyntaxNode<'a>, kind: SyntaxKind) -> Vec<SyntaxNode<'a>> {
    node.children().filter(|n| n.kind() == kind).collect()
}

/// Queue `nodes` to be visited in order.
fn visit_in_order<'a>(stack: &mut Vec<Step<'a>>, nodes: Vec<SyntaxNode<'a>>, in_fn: bool) {
    stack.extend(nodes.into_iter().rev().map(|n| Step::Visit(n, in_fn)));
}

/// The start and text of the first `Name` token of `node` — the root of a name
/// chain (`a` of `a.b:c`).
fn root_name(node: SyntaxNode<'_>) -> Option<(u32, &str)> {
    node.descendants_with_tokens().find_map(|c| match c {
        NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => Some((u32::from(t.text_range().start()), t.text())),
        _ => None,
    })
}

/// The call `node` (a `local` statement) initializes its one name with, when it
/// declares no type.
fn initializing_call(node: SyntaxNode<'_>) -> Option<FunctionCall<'_>> {
    let assign = LocalAssign::cast(node)?;
    let [Expression::FunctionCall(call)] = assign.expression_list()?.expressions()[..] else { return None };
    if assign.name_list()?.names().len() != 1 {
        return None;
    }
    let annotations = super::extract_annotations(node);
    (annotations.class.is_none() && annotations.var_type.is_none() && super::extract_inline_class(node).is_none())
        .then_some(call)
}

impl<'a> FunctionLocals<'a> {
    /// Walk the file `root`, declaring names where the build does: a `local`'s
    /// after its values, a `local function`'s before its body, a function's
    /// parameters (and a method's `self`) and a loop's variables in their bodies.
    pub fn collect(root: SyntaxNode<'a>) -> Self {
        let mut call_locals: Vec<CallLocal<'a>> = Vec::new();
        let mut bindings: HashMap<u32, FunctionBinding> = HashMap::default();
        let mut ordinals: HashMap<&'a str, usize> = HashMap::default();
        // Each declared name's bindings in scope, innermost last, and the names
        // each open scope declares.
        let mut visible: HashMap<&'a str, Vec<FunctionBinding>> = HashMap::default();
        let mut scopes: Vec<Vec<&'a str>> = Vec::new();
        let mut stack = vec![Step::Visit(root, false)];
        while let Some(step) = stack.pop() {
            let (node, in_fn) = match step {
                Step::Visit(node, in_fn) => (node, in_fn),
                Step::Declare(name, binding) => {
                    visible.entry(name).or_default().push(binding);
                    if let Some(scope) = scopes.last_mut() {
                        scope.push(name);
                    }
                    continue;
                }
                Step::OpenScope => {
                    scopes.push(Vec::new());
                    continue;
                }
                Step::CloseScope => {
                    for name in scopes.pop().unwrap_or_default() {
                        if let Some(names) = visible.get_mut(name) {
                            names.pop();
                        }
                    }
                    continue;
                }
            };
            let resolve = |name: &str| visible.get(name).and_then(|b| b.last()).copied();
            match node.kind() {
                SyntaxKind::Block => {
                    stack.push(Step::CloseScope);
                    visit_in_order(&mut stack, node.children().collect(), in_fn);
                    stack.push(Step::OpenScope);
                }
                SyntaxKind::LocalAssignStatement => {
                    // File locals aren't tracked: a function's own names shadow them.
                    if in_fn {
                        let names: Vec<&'a str> = child_nodes(node, SyntaxKind::NameList).into_iter()
                            .flat_map(|list| child_tokens(list, SyntaxKind::Name))
                            .collect();
                        let call = initializing_call(node);
                        for &name in names.iter().rev() {
                            let binding = match call {
                                Some(call) => {
                                    let ordinal = ordinals.entry(name).or_default();
                                    *ordinal += 1;
                                    call_locals.push(CallLocal {
                                        key: format!("{name}#{ordinal}"),
                                        call,
                                        rebound: false,
                                    });
                                    FunctionBinding::Call(call_locals.len() - 1)
                                }
                                None => FunctionBinding::Other,
                            };
                            stack.push(Step::Declare(name, binding));
                        }
                    }
                    // The values are evaluated before the names are declared.
                    visit_in_order(&mut stack, child_nodes(node, SyntaxKind::ExpressionList), in_fn);
                }
                SyntaxKind::AssignStatement => {
                    for target in child_nodes(node, SyntaxKind::VariableList).into_iter().flat_map(|list| child_nodes(list, SyntaxKind::NameRef)) {
                        if let Some((_, name)) = root_name(target)
                            && let Some(FunctionBinding::Call(i)) = resolve(name)
                        {
                            call_locals[i].rebound = true;
                        }
                    }
                    visit_in_order(&mut stack, node.children().collect(), in_fn);
                }
                SyntaxKind::FunctionDefinition => {
                    let is_local = node.children_with_tokens()
                        .any(|c| matches!(c, NodeOrToken::Token(t) if t.kind() == SyntaxKind::LocalKeyword));
                    let name = child_tokens(node, SyntaxKind::Name).first().copied();
                    let path = child_nodes(node, SyntaxKind::DotAccess).first().copied();
                    match (name, path) {
                        // `function f()` assigns `f`.
                        (Some(name), _) if !is_local => {
                            if let Some(FunctionBinding::Call(i)) = resolve(name) {
                                call_locals[i].rebound = true;
                            }
                        }
                        (None, Some(path)) => {
                            if let Some((start, root)) = root_name(path)
                                && let Some(binding) = resolve(root)
                            {
                                bindings.insert(start, binding);
                            }
                        }
                        _ => {}
                    }
                    let is_method = path.is_some_and(|p| !child_tokens(p, SyntaxKind::Colon).is_empty());
                    stack.push(Step::CloseScope);
                    visit_in_order(&mut stack, child_nodes(node, SyntaxKind::Block), true);
                    let params: Vec<&'a str> = child_nodes(node, SyntaxKind::ParameterList).into_iter()
                        .flat_map(|list| child_tokens(list, SyntaxKind::Parameter))
                        .collect();
                    stack.extend(params.into_iter().rev().map(|p| Step::Declare(p, FunctionBinding::Other)));
                    if is_method {
                        stack.push(Step::Declare("self", FunctionBinding::Other));
                    }
                    stack.push(Step::OpenScope);
                    // `local function f` declares `f` in the enclosing scope, before its body.
                    if is_local && in_fn && let Some(name) = name {
                        stack.push(Step::Declare(name, FunctionBinding::Other));
                    }
                }
                SyntaxKind::ForCountLoop | SyntaxKind::ForInLoop => {
                    stack.push(Step::CloseScope);
                    visit_in_order(&mut stack, child_nodes(node, SyntaxKind::Block), in_fn);
                    if in_fn {
                        let vars: Vec<&'a str> = child_tokens(node, SyntaxKind::Name).into_iter()
                            .chain(child_nodes(node, SyntaxKind::NameList).into_iter().flat_map(|list| child_tokens(list, SyntaxKind::Name)))
                            .collect();
                        stack.extend(vars.into_iter().rev().map(|v| Step::Declare(v, FunctionBinding::Other)));
                    }
                    stack.push(Step::OpenScope);
                    // The bounds and the iterator are evaluated outside the loop.
                    visit_in_order(&mut stack, node.children().filter(|n| n.kind() != SyntaxKind::Block).collect(), in_fn);
                }
                SyntaxKind::NameRef => {
                    if let Some((start, name)) = root_name(node)
                        && let Some(binding) = resolve(name)
                    {
                        bindings.insert(start, binding);
                    }
                }
                _ => visit_in_order(&mut stack, node.children().collect(), in_fn),
            }
        }
        Self { call_locals, bindings }
    }

    /// What the name chain `node` (an identifier, or a `function` statement's
    /// dotted name) is rooted at, when a function declares that name.
    pub fn root_binding(&self, node: SyntaxNode<'_>) -> Option<FunctionBinding> {
        root_name(node).and_then(|(start, _)| self.bindings.get(&start).copied())
    }

    /// The call local `node` is rooted at, unless it is assigned again.
    pub fn call_local_root(&self, node: SyntaxNode<'_>) -> Option<usize> {
        match self.root_binding(node)? {
            FunctionBinding::Call(i) if !self.call_locals[i].rebound => Some(i),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_resolve_to_the_function_scope_declaring_them() {
        let src = "\
local f = CreateFrame('Frame')
function A(p)
    local f = CreateFrame('Frame')
    f.a = 1
    do
        local f = 1
        f.b = 2
    end
    for _, f in pairs(p) do f.c = 3 end
    local function g(f) f.d = 4 end
    local alias = f
    local r = CreateFrame('Frame')
    p:HookScript('OnShow', function() r = nil end)
end
f.e = 5
";
        let tree = crate::syntax::parser::parse(src);
        let locals = FunctionLocals::collect(SyntaxNode::new_root(&tree));
        let at = |pat: &str| locals.bindings.get(&(src.find(pat).unwrap() as u32)).copied();
        assert!(matches!(at("f.a"), Some(FunctionBinding::Call(i)) if locals.call_locals[i].key == "f#1"));
        // An inner `local`, a loop variable and a parameter shadow it.
        for pat in ["f.b", "f.c", "f.d"] {
            assert!(matches!(at(pat), Some(FunctionBinding::Other)), "{pat}");
        }
        assert!(matches!(at("f\n    local r"), Some(FunctionBinding::Call(_))));
        // A file local isn't a function's.
        assert!(at("f.e").is_none());
        // Assigned again in a closure: no longer only the call's value.
        assert!(locals.call_locals.iter().any(|l| l.key == "r#1" && l.rebound));
        assert!(locals.call_locals.iter().all(|l| l.key != "alias#1"));
    }
}
