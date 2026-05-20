-- Cross-file bare self-field test: fields assigned from params and literals
-- without explicit ---@type annotations.

---@class BareDB
---@field query fun(): string

---@class BareFieldClass
local BFC = {}

---@param db BareDB
---@param label string
function BFC:Init(db, label)
    self.db = db
    self.label = label
    self.ready = true
    self.data = {}
    self.count = 0
    -- Table literal assigned to self-field: field names should be preserved
    self.options = {
        scale = self:MakeOptions(),
        name = "hello",
    }
end

--- @return BareDB
function BFC:MakeOptions()
    return {}
end
