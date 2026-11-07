-- A class's only Lua file writes members onto a field's value: it reads the
-- field with them carried, as other files do.
---@class ClassWriterPanel
local Panel = {}
_G.ClassWriterPanel = Panel

function Panel:Init()
    local header = CreateFrame("Frame")
    header.extra = 5
    self.header = header
end

function Panel:Extra()
    local header = self.header
    --    ^ hover: (local) header: Frame & { extra: number } {
    return header.extra
end
