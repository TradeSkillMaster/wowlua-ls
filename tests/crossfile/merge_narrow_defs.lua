-- Cross-file source for the deferred sibling-narrowing-through-merge fix. A
-- no-@return method whose body yields correlated cases `(number, number)` and
-- `(nil, nil)`. Cross-file callers resolve this lazily (deferred), so the
-- sibling narrowing runs in Phase 2 after the post-block BranchMerge was built.
---@class MergeNarrowSource
local MergeNarrowSource = {}

function MergeNarrowSource:Range(key)
    if not key then
        return
    end
    return key + 1, key + 2
end

function MergeNarrowSource:Search(field, value)
    return 1
end

function MergeNarrowSource:Count()
    return 0
end
