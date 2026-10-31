-- `constants.lua`'s namespace data read from another file, also where nothing
-- else in the file resolves the read (a key into an untyped table).
---@diagnostic disable: unused-local, undefined-field
local ns = select(2, ...)

ns.Widgets.Gauge = {}

function ns.Widgets.Gauge:SetUnit()
    local kind = UnitPowerType(self.unit)
    if self.details.kinds[ns.Constants.KindNames[kind]] then
    --                                 ^ hover: (field) KindNames: string[]
        self.kind = kind
    end
end

-- Members this file adds to `constants.lua`'s table stay on it.
local toolkit
if ns.Toolkit then
    toolkit = ns.Toolkit
else
    toolkit = {}
end
toolkit.state = {}
function toolkit:Start() end
local state = toolkit.state
--    ^ hover: (local) state: table
