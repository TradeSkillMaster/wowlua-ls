---@param seg CorrSegment
---@param lines string[]
local function register(seg, lines)
    if not seg.slot then
        lines[#lines + 1] = seg.label
        seg.slot = #lines
        seg.tags = {}
        seg.count = 1
    else
        -- The falsy `not seg.slot` branch narrows its siblings.
        seg.count = seg.count + 1
    end
    if seg.tags ~= nil then
        -- A `~= nil` guard on one sibling narrows the others.
        lines[seg.slot] = seg.label .. #seg.tags
    end
end

---@param seg CorrSegment
local function truthy(seg)
    if seg.tags then
        return seg.count + 1
    end
    return 0
end

-- The group reaches a subclass declared in the same file as the parent.
---@param seg CorrSubSegment
local function sub(seg)
    if seg.slot then
        return seg.count + 1
    end
    return 0
end

-- Groups from both declarations of a class apply.
---@param p CorrPair
local function pair(p)
    if p.left and p.top then
        return p.right + p.bottom
    end
    return 0
end

-- A guard on a field outside the group narrows none of it.
---@param seg CorrSegment
local function unrelated(seg)
    if seg.label then
        return seg.count + 1
        --     ^ diag: invalid-op
    end
    return 0
end
