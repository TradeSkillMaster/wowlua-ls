-- This file declares `CorrPair` again; the groups the other declarations
-- (defs.lua, defs2.lua) give it still apply here.
---@class CorrPair
---@field middle integer?

---@param p CorrPair
local function pair(p)
    if p.left and p.top then
        return p.right + p.bottom
    end
    return 0
end

-- A subclass this file declares of a class from another file inherits its groups.
---@class CorrLocalSub : CorrSegment

---@param seg CorrLocalSub
local function localSub(seg)
    if seg.slot then
        return seg.count + 1
    end
    return 0
end

-- A guard outside every group narrows nothing.
---@param p CorrPair
local function unrelated(p)
    if p.middle then
        return p.right + 1
        --     ^ diag: invalid-op
    end
    return 0
end
