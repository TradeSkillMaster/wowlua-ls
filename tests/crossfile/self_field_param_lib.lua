-- Cross-file self-field test: class with bare self-fields accessed via @param

---@class ParamFieldClass
local Mixin = {}

--- @param db table
--- @param label string
function Mixin:Init(db, label)
    self.db = db
    self.label = label
    self.opts = {
        scale = 1.0,
    }
end

function Mixin:DoWork()
    return self.db
end
