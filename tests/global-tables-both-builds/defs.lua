---@diagnostic disable: create-global
-- Global tables shaped like FrameXML's, read from user.lua: the stub build (the
-- cold `build`) types them as the workspace build does.

-- A table constructor's named entries are the global's fields.
PanelConstants = {
    WrapText = true,
    Padding = 4,
    Insets = { Left = 2 },
    Pending = nil,
}

-- A typed write replaces a constructor entry.
PanelConstants.Pending = "ready"

-- A string key names an entry too; other keys aren't scanned, so a dynamic index
-- into a constructor that has them stays untyped.
PanelKeys = { ["Mode"] = 1, Label = "x" }
PanelFactions = { [0] = "Horde", [1] = "Alliance", Horde = 0, Alliance = 1 }

---@class PanelFormatter
PanelFormatter = {}

---@return string
function PanelFormatter:Format(seconds)
    return ""
end

---@return PanelFormatter
function CreatePanelFormatter()
    return PanelFormatter
end

-- A call-valued entry is typed from its callee's return.
PanelDefaults = { Formatter = CreatePanelFormatter() }

-- A method defined through a field assigned a call belongs to the call's object,
-- not to the table holding the field: on a class or a plain table.
---@class PanelMixin
PanelMixin = {}
PanelMixin.TimeFormatter = CreatePanelFormatter()

---@return number
function PanelMixin.TimeFormatter:GetMinInterval(seconds)
    return seconds
end

PanelUtil = {}
PanelUtil.TimeFormatter = CreatePanelFormatter()

---@return boolean
function PanelUtil.TimeFormatter:IsShort()
    return true
end

-- The call's class also when it may return nil, or is an intersection.
---@class PanelStyle
---@field padding number

---@return PanelFormatter?
function FindPanelFormatter()
    return PanelFormatter
end

---@return PanelFormatter & PanelStyle
function CreateStyledFormatter()
    return CreateStyledFormatter()
end

PanelMixin.FoundFormatter = FindPanelFormatter()
PanelMixin.StyledFormatter = CreateStyledFormatter()

---@return number
function PanelMixin.FoundFormatter:GetSpan()
    return 1
end

---@return boolean
function PanelMixin.StyledFormatter:IsStyled()
    return true
end

-- A generic call binds the generics of its callee from the arguments that name
-- a global or a field path: `CreateFromMixins(M)` is an `M`.
---@generic ...M
---@param ... ...M
---@return & ...M
function CreatePanelFromMixins(...) end

PanelMixin.CountFormatter = CreatePanelFromMixins(PanelFormatter)

---@return number
function PanelMixin.CountFormatter:GetUnitCount()
    return 1
end

PanelUtil.CountFormatter = CreatePanelFromMixins(PanelFormatter)

-- So does a constructor entry's call.
PanelPresets = { Formatter = CreatePanelFromMixins(PanelFormatter) }

-- An argument naming a plain table binds nothing: the field would be that table.
PanelUtil.Copy = CreatePanelFromMixins(PanelConstants)

-- A method call's arguments follow its receiver.
PanelFactory = {}

---@generic T
---@param proto T
---@return T
function PanelFactory:Clone(proto)
    return proto
end

PanelUtil.ClonedFormatter = PanelFactory:Clone(PanelFormatter)
