-- Test: correlated return-only overload inference disabled via config
-- With `inference.correlated_return_overloads: false` (the default), no
-- synthesized overloads are added, so sibling narrowing doesn't fire.
-- The base return types are still recovered from `func.rets` (each return
-- slot's type is the union across every `return` statement, including those
-- in nested branches), so callers see `string | nil` / `number | nil`
-- rather than `?`.

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
    -- No synthesis → no overload-driven sibling narrowing — `b` keeps its
    -- raw `number | nil` even though `a` is narrowed by the `if`.
    local _ = a
    --        ^ hover: (local) a: string
    local _ = b
    --        ^ hover: (local) b: nil | number
end
