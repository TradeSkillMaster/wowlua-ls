-- A function reading a local declared outside it sees every value the file
-- assigns the local, not only the one visible where the function is defined.

local _, ns = ...

-- Declared without a value, assigned in one function, read in another: the
-- reads take the assigned value and aren't nilable.
local db

function ns.Init()
    db = { profile = { size = 1 } }
end

function ns.Use()
    local s = db.profile.size
    --    ^ hover: (local) s: number
    return s
end

-- Filled lazily by the function reading it, and read in another.
local pending

function ns.Queue(item)
    pending = pending or {}
    --        ^ hover: (local) pending: table
    pending[#pending + 1] = item
end

function ns.Peek()
    local p = pending
    --    ^ hover: (local) p: table
    return p
end

-- Reads leave the declaration's nil out, but checking for it isn't redundant.
local cache

local function GetCache()
    if not cache then
        cache = {}
    end
    return cache
end

-- `local x = nil` declares a value no more than `local x` does.
local MT = nil
MT = {
    fallback = { size = 1 },
    __index = function(_, key)
        return MT.fallback[key]
    end,
}

-- A function assigning nil makes the reads nilable.
---@class UpvalueWidget
local UpvalueWidget = {}
function UpvalueWidget:Refresh() end

---@return UpvalueWidget
local function NewWidget()
    return setmetatable({}, { __index = UpvalueWidget })
end

local widget

function ns.CreateWidget()
    widget = NewWidget()
end

function ns.DestroyWidget()
    widget = nil
end

function ns.RefreshWidget()
    widget:Refresh()
    --     ^ diag: need-check-nil
end

-- A forward-declared local function, defined after its caller.
local Helper

local function Caller()
    return Helper(1)
    --     ^ hover: (local) function Helper(n: number)
end

function Helper(n)
    return n + 1
end

-- A function built while computing the local's value reads that value when the
-- local had none before.
---@class UpvalueTicker
local UpvalueTicker = {}
function UpvalueTicker:Cancel() end

---@param callback fun()
---@return UpvalueTicker
local function NewTicker(callback)
    return setmetatable({ callback = callback }, { __index = UpvalueTicker })
end

local ticker
ticker = NewTicker(function()
    ticker:Cancel()
    -- ^ hover: (local) ticker: UpvalueTicker
end)

-- A list reset in the function draining it keeps the element type of the
-- values another function inserts.
local queue = {}

function ns.Push(label)
    table.insert(queue, { label = tostring(label) })
end

function ns.Drain()
    for _, entry in ipairs(queue) do
        print(entry.label)
        --          ^ hover: (field) label: string
    end
    queue = {}
end

-- An annotated local keeps its declared type.
---@class UpvalueSettings
---@field scale number

---@type UpvalueSettings
local settings

---@param saved table
function ns.LoadSettings(saved)
    settings = saved
end

function ns.Scale()
    return settings.scale
    --     ^ hover: (local) settings: UpvalueSettings
end

-- A later `local` of the same name is a different variable: writes to it don't
-- reach reads of the first one.
local mode = 1

local function GetMode()
    return mode
    --     ^ hover: (local) mode: number
end

function ns.ResetMode()
    mode = 2
end

local mode = "a"
--    ^ diag: redefined-local

function ns.SetMode()
    mode = "b"
end

-- Writes to a same-named local of another function don't reach this one: the
-- record is still never reassigned.
local record = { a = 1 }

local function ReadRecord()
    return record.b
    --            ^ diag: undefined-field
end

local function Other()
    local record
    --    ^ diag: shadowed-local
    record = {}
    return record
end

-- A function in a `local` statement's value assigns the local that statement
-- declares, not an outer one of the same name.
local session = { id = 1 }

local function ReadSession()
    return session.token
    --             ^ diag: undefined-field
end

---@param callback function
local function Wrap(callback)
    return callback
end

function ns.Restart()
    local session = Wrap(function()
    --    ^ diag: shadowed-local
        session = { token = "t" }
    end)
    local function Token()
        return session
        --     ^ hover: (local) session: function | {token: string}
    end
    return Token
end

-- Defining a method on a local reads it.
local tooling

function ns.InitTooling()
    tooling = { level = 1 }
end

function ns.SetupTooling()
    function tooling.describe()
        return "tooling"
    end
end

function ns.DescribeTooling()
    return tooling.describe()
    --             ^ hover: (field) function describe()
end

-- A function built in the value of a `local` repeating a name reads the first
-- variable, which another function assigns.
local level = 1

function ns.SetLevel()
    level = "high"
end

local level = Wrap(function()
--    ^ diag: redefined-local
    local current = level
    --    ^ hover: (local) current: number | string
    return current
end)

-- Checking such a local after an early return guarded by `x and …` isn't
-- redundant either.
local toggled

function ns.Toggle()
    if toggled and toggled:IsShown() then
        toggled:Hide()
        return
    end
    if not toggled then
        toggled = CreateFrame("Frame")
    end
    toggled:Show()
end

-- A write whose value resolves after the reading function's entry still types it.
local store

local function ReadStore()
    local v = store.x
    --    ^ hover: (local) v: number
    return v
end

local function LoadStore()
    return { x = 1 }
end

function ns.InitStore()
    store = LoadStore()
end

-- A set filled lazily with numeric keys doesn't type the keys it's looked up with.
local allowedKeys = nil

local function GetAllowedKeys()
    if allowedKeys then
        return allowedKeys
    end
    allowedKeys = {}
    for i = 1, 3 do
        allowedKeys[i] = true
    end
    return allowedKeys
end

local function IsAllowedKey(commandKey)
    return GetAllowedKeys()[commandKey] == true
end

IsAllowedKey("WHO")

print(GetCache, Caller, GetMode, ReadRecord, Other, ReadSession, ReadStore, level)
