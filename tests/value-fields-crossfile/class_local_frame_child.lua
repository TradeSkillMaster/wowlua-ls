-- A `---@class` local's field holding a frame: a member written through it in a
-- method types that member in the file writing it.
---@class GripMixin
local GripMixin = {}

function GripMixin:Init(minWidth)
    self.minWidth = minWidth
end

---@class GripFrame
local GripFrame = {}

---@return GripFrame & GripMixin
local function CreateGrip() return GripFrame --[[@as GripFrame & GripMixin]] end

local function BuildPanel()
    ---@class SizedPanel
    local panel = {}
    local grip = CreateGrip()
    panel.Grip = grip

    function panel:Fit()
        local width = 40
        self.Grip.minWidth = width
        --        ^ hover: (field) minWidth: number
    end
    return panel
end

-- The write goes to the member the intersection already has, on `GripMixin`: a
-- `GripFrame` alone still has no `minWidth`.
---@param f GripFrame
local function FrameWidth(f)
    return f.minWidth
    --       ^ diag: undefined-field
end

-- A new member goes on the intersection's class of this file, never on the stub
-- class: another `Frame` here has no `extraWidth`.
---@return Frame & GripMixin
local function CreateStubGrip() return Mixin(CreateFrame("Frame"), GripMixin) end

local function BuildStubPanel()
    ---@class StubSizedPanel
    local panel = {}
    panel.Grip = CreateStubGrip()
    function panel:Fit() self.Grip.extraWidth = 40 end
    return panel
end

local plain = CreateFrame("Frame")
local plainWidth = plain.extraWidth
--                       ^ diag: undefined-field

return BuildPanel, FrameWidth, BuildStubPanel, plainWidth
