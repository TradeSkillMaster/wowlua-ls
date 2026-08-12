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

-- Body-inferred ARRAY return: the deferred cross-file lift carries the array
-- element type inline (`string[]`) instead of decaying the anonymous table to
-- `any` (Stage 0 of the lossless-cross-file work).
function ns.Core.GetItems()
    return { "a", "b", "c" }
end

-- Body-inferred MAP return (via `@type` on the returned local): the lift carries
-- both key and value element types (`table<string, number>`).
function ns.Core.GetLookup()
    ---@type table<string, number>
    local m = {}
    return m
end

-- Body-inferred RECORD return: the deferred cross-file lift carries each named
-- field's resolved type inline (`{ label: string, x: number, y: number }`)
-- instead of decaying the anonymous table to `any` (record-return lift). Fields
-- render sorted by name.
function ns.Core.GetPoint()
    return { x = 1, y = 2, label = "origin" }
end

-- Nested record return: a field that is itself a record recurses through the
-- lift, so the inner shape is carried too.
function ns.Core.GetPlacement()
    return { name = "spawn", pos = { x = 1, y = 2, label = "origin" } }
end

-- Record field carrying an explicit `@type` annotation: the annotated type is
-- preferred over the inferred RHS (here a bare `{}` that would otherwise be
-- `any`).
function ns.Core.GetTagged()
    ---@type string[]
    local names = {}
    return { count = 0, names = names }
end

-- Body-inferred MAP with an INFERRED (non-annotated) string key. resolve.rs sets
-- key_type WITHOUT setting is_explicit_map for inferred maps, so the lift must
-- decide array-vs-map from the key type (non-`Number` → map), not from that flag
-- — otherwise this collapses to `number[]` cross-file (wrong: claims integer
-- indexing on a string-keyed table).
---@param k string
function ns.Core.GetInferredMap(k)
    local m = { [k] = 1 }
    return m
end
