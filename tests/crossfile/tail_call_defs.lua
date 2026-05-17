-- Cross-file tail-call return type test: definitions
-- Functions whose only return is a tail call to another function.
-- The callee's return arity should propagate through.

local addonName, ns = ...

ns.Helpers = {}

local private = {}

-- Callee with multi-return (concrete types visible at AST level)
function private.GetResult()
    return "value", 99
end

-- Single-path tail-call wrapper: resolves the callee within the same
-- file to inherit its concrete return types cross-file.
function ns.Helpers.Fetch()
    return private.GetResult()
end
