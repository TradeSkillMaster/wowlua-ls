-- Round 2 of signatures: returns, receivers and generics other files read as
-- this file types them.
local _, ns = ...

---@class SigMod
local proto = {}
local mt = { __index = proto }

function proto:GetName()
    return "mod"
end

-- An empty table on one path, a class instance on another.
function ns.NewMod(name)
    if name == "" then
        return {}
    end
    ---@class SigMod
    local obj = setmetatable({ id = name }, mt)
    return obj
end

-- A table whose metatable has neither `__index` nor `__call` is a plain table.
function ns.OrderedTable()
    local meta = {}
    function meta:__newindex(key, value)
        rawset(self, key, value)
    end
    return setmetatable({}, meta)
end

-- A parameter returned as passed is the function's own generic.
function ns.Recover(info, link, quality)
    if not info then
        return link, quality
    end
    return "link", 1
end

-- Returns an author annotates.
---@return string info
---@return boolean isNew
function ns.GetInfo(id)
    return id, true
end

-- A library's methods calling each other through `self`, the library's class.
local Lib = LibStub:NewLibrary("SigLib-1.0", 1)

function Lib:Base()
    local frame = CreateFrame("Frame")
    frame.label = "x"
    return frame
end

function Lib:WithWindow()
    local frame = self:Base()
    frame.button = 1
    return frame
end

-- A function defined inside another (an initializer).
function ns.Init()
    local frames = {}
    function ns.IsOpen()
        return next(frames) ~= nil
    end
end

-- A method of the object a call makes.
local toggle = CreateFrame("Button", "SigToggleButton", UIParent, "UIPanelButtonTemplate")
ns.Toggle = toggle
function ns.Toggle:SetTip(text)
    self.tip = text
end

-- A return typed by a missing type marked optional keeps its slot.
---@return SigNoSuchInfo? info
---@return boolean isNew
function ns.GetEntry(id)
    return id, true
end

-- A field typed so stays the class's.
---@class SigEntry
---@field opt SigNoSuchOpt?
ns.Entry = {}
