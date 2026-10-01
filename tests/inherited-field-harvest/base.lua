---@class Base
local Base = {}

function Base:__init()
    local header = MakeWidget()
    self._header = header
    -- Sub-fields written onto the field's value: through the field chain and
    -- through the local alias. Carried to readers in other files as a shape.
    self._header.cells = {}
    self._header.count = 3
    header.extra = "x"
    self._bindings = MakeBindings()
end
