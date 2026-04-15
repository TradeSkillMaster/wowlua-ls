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
end
