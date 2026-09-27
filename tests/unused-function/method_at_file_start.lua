function NS.DefinedAtFileStartUnused()
    return 80
end

-- The method above starts at byte 0. `cb()` below resolves to the function
-- materialized for the `fun()` annotation, which has no definition node and a
-- dummy offset of 0: that must not count as a reference to the method.
---@param cb fun(): number
local function invoke(cb)
    return cb()
end
invoke(function() return 2 end)
