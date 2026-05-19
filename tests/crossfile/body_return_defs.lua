-- Cross-file body-inferred return type test: definitions
-- Functions without @return annotations whose return types are
-- inferred from their body expressions.

local addonName, ns = ...

ns.Core = {}

local cache = {}

-- Multi-return with comparison: should infer (any, boolean)
function ns.Core.GetCachedItem(key)
    local val = cache[key]
    return val, val ~= nil
end

-- Single boolean return via comparison
function ns.Core.HasItem(key)
    return cache[key] ~= nil
end

-- Single boolean return via `not`
function ns.Core.IsEmpty()
    return not cache["default"]
end

-- Returns with literals: (string, number, boolean)
function ns.Core.GetDefaults()
    return "default", 42, true
end

-- Multi-return paths: if/else with different arities
-- (should pick max-arity return)
function ns.Core.TryGet(key)
    if cache[key] then
        return cache[key], true
    end
    return nil, false
end

-- Multi-path with different concrete types at the same position
-- (should widen first return to any since string ~= number)
function ns.Core.Classify(key)
    if cache[key] then
        return "found", true
    end
    return 0, false
end

-- Comparison in parenthesized expression
function ns.Core.CheckWrapped(a, b)
    return (a == b)
end
