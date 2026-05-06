-- Cross-file self-field test: consumer re-declares class with a different variable name
-- Verifies that typed self-fields from the lib file are visible here.

---@class SFWidget
local Widget = {}

function Widget:Render()
    -- Access typed self-field set in the lib's method (variable name differs: W vs Widget)
    local l = self._label
    --                ^ hover: (field) _label: string  def: external
    -- Access @field from the class declaration
    local n = self.name
    --             ^ hover: (field) name: string  def: external
end
