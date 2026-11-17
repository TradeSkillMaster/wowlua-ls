-- `@correlated` field groups declared here narrow the fields of these classes
-- in user.lua.

---@class CorrSegment
---@field label string
---@field slot integer?
---@field tags string[]?
---@field count integer?
---@correlated slot, tags, count

---@class CorrSubSegment : CorrSegment
---@field extra boolean?

-- A second declaration of the class, in another file, adds a group.
---@class CorrPair
---@field left integer?
---@field right integer?
---@correlated left, right
