-- Cross-file self-field test: child class accessing parent's typed self-fields

---@class SFChild : SFBase
local SFChild = {}

function SFChild:DoWork()
    -- Access inherited typed self-fields from parent class methods
    local d = self._data
    --                ^ hover: (field) _data: SFQuery!  def: external
    local l = self._label
    --                ^ hover: (field) _label: string  def: external
    -- Access own @field from parent
    local n = self.name
    --             ^ hover: (field) name: string  def: external
    -- Spaced annotation (--- @type with space after ---)
    local s = self._spaced
    --                ^ hover: (field) _spaced: SFQuery  def: external
end

-- Cross-file self-field test: global variable with @class name different from var name
---@type SFGlobalClass
local gc = {}

local gdb = gc.db
--             ^ hover: (field) db: table  def: external
local gtag = gc.tag
--               ^ hover: (field) tag: string  def: external
