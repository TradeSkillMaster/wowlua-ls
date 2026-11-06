-- `CarryTable.header` is written here; `table_b.lua`, another file of the class,
-- writes a field onto its value too.
---@class CarryHeader
---@field title string

---@class CarryTable
CarryTable = {}

---@param h CarryHeader
function CarryTable:Init(h)
    self.header = h
    self.header.cells = {}
end

function CarryTable:Header()
    local hdr = self.header
    return hdr
end
