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
