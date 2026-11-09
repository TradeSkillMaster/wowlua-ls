-- Signatures other files read as this file types them: the implicit `self`,
-- parameters backward inference types, and returns.
local _, ns = ...

---@class SigWidget
local SigWidget = {}
ns.SigWidget = SigWidget

function SigWidget:SetMargin(left, right)
    self.left = left + 1
    self.right = right .. "px"
    return self
end

---@class SigPanel
local SigPanel = {}
ns.SigPanel = SigPanel

-- `@param self` types the implicit `self`.
---@param self SigWidget | SigPanel
function SigWidget:Owner()
    return self
end

function ns.Format(value)
    return value .. "!"
end

---@class SigState
local SigState = {}
ns.SigState = SigState

---@class SigSchema
local SigSchema = {}
ns.SigSchema = SigSchema

---@return built: SigState
function SigSchema:CreateState()
    return SigState
end

-- A returned frame carrying fields written through it, beside an implicit `nil`.
function ns.MakeThing(flag)
    if flag then
        local f = CreateFrame("Frame")
        f.extra = 1
        return f
    end
end

-- A template the call can't type: the frame's intersection with `any`, nil beside.
function ns.MakeOther(parent, flag)
    if not flag then return nil end
    return CreateFrame("Frame", nil, parent, "UnknownTemplate")
end
