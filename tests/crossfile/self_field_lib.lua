-- Cross-file self-field test: parent class with typed self-field assignments in methods

---@class SFBase
---@field name string
local SFBase = {}

---@class SFQuery
---@field results table

function SFBase:Initialize()
    self._data = nil ---@type SFQuery!
    ---@type string
    self._label = ""
end

-- Cross-file self-field test: global variable with different @class name
---@class SFGlobalClass
SFGlobalMixin = {}

---@param db table
function SFGlobalMixin:Init(db)
    self.db = db
    ---@type string
    self.tag = "default"
end
