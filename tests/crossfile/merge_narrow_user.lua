---@diagnostic disable: unused-local, unused-function
-- Regression: cross-file (deferred) correlated sibling narrowing must propagate
-- through the post-block BranchMerge. `firstIndex`/`lastIndex` are forward-
-- declared, assigned via the deferred multi-return in the `if` branch (where an
-- early-exit guard narrows both siblings to `number`), and assigned via the
-- or-idiom in the `else` branch. At the merge, `lastIndex` must be `number`
-- (not `number?`), so `lastIndex - firstIndex` does NOT emit `invalid-op`.

---@param db MergeNarrowSource
---@param field string
---@param valueMin number?
---@param valueMax number?
local function opt(db, field, valueMin, valueMax)
    local count = db:Count()
    local firstIndex, lastIndex = nil, nil
    if valueMin and valueMax and valueMin == valueMax then
        firstIndex, lastIndex = db:Range(valueMin)
        if not firstIndex then
            return
        end
    else
        firstIndex = valueMin and db:Search(field, valueMin) or 1
        lastIndex = valueMax and db:Search(field, valueMax) or count
    end
    local indexDiff = lastIndex - firstIndex
    --                ^ hover: (local) lastIndex: number
    local _ = indexDiff
end
