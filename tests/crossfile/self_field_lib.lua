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
    --- @type SFQuery
    self._spaced = nil
end

-- A multi-target `@type` list types each self-field by position.
local function sfPair() return nil, nil end

function SFBase:InitPairs()
    ---@type number, SFQuery
    self._count, self._query = sfPair()
    self._left, self._right = nil, nil ---@type string, SFQuery
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
