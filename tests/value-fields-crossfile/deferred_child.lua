-- A field written through a `---@class` local after a method reading it is
-- defined types that read in the file writing it.
local function Build()
    ---@class ChildPanel: Frame
    local panel = CreateFrame("Frame")

    function panel:GetHeader()
        local header = self.Header
        --    ^ hover: (local) header: ChildPanel.Header {
        return header
    end

    ---@class ChildPanel.Header: Frame
    local header = CreateFrame("Frame")
    panel.Header = header
end

-- So does a `self` field one of whose writes is such a local, read through an
-- alias or an `or` too.
---@class MixedHost
local Mixed = {}

function Mixed:Read()
    local x = self.mix
    --    ^ hover: (local) x: string | number
    local y = x or 0
    --    ^ hover: (local) y: string | number
    return y
end

function Mixed:SetName()
    local name = Mixed.MakeName()
    self.mix = name
end

function Mixed:SetCount()
    self.mix = 1
end

---@return string
function Mixed.MakeName() return "a" end

return Build, Mixed
