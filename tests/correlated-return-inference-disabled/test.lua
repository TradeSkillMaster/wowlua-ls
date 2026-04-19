-- Test: correlated return-only overload inference disabled via config
-- With `inference.correlated_return_overloads: false` (the default), no
-- synthesized overloads are added. The function call's return type isn't
-- inferred from nested-scope returns either, so callers see `?`.

local cond = true

local function pair()
    if cond then
        return "alice", 42
    else
        return nil, nil
    end
end

local a, b = pair()
if a then
    -- No synthesis → no inferred overloads → no base return type → `?`.
    -- Sibling narrowing also doesn't fire.
    local _ = a
    --        ^ hover: (global) a: ?
    local _ = b
    --        ^ hover: (global) b: ?
end
