---@diagnostic disable: unused-local, unused-function
-- Calls to the `@noreturn` functions defs.lua declares leave the block.

local _, ns = ...

---@return number
local function viaGlobal(x)
    if x then return 1 end
    NoReturnCrossfile_Fail("no value")
end

---@param s string?
local function narrowViaGlobal(s)
    if not s then NoReturnCrossfile_Fail("no value") end
    return s
    --     ^ hover: (param) s: string
end

---@param s string?
local function narrowViaNamespace(s)
    if not s then ns.Errors.Fail("no value") end
    return s
    --     ^ hover: (param) s: string
end

---@return number
local function viaNamespace(x)
    if x then return 1 end
    ns.Errors.Fail("no value")
end

---@param s string?
local function narrowViaClassTable(s)
    if not s then NoReturnCrossfileLogger:Fatal("no value") end
    return s
    --     ^ hover: (param) s: string
end

---@param logger NoReturnCrossfileLogger
---@param s string?
local function narrowViaInstance(logger, s)
    if not s then logger:Fatal("no value") end
    return s
    --     ^ hover: (param) s: string
end

-- Untagged: the call returns as far as this file knows.
---@return number
local function viaUntagged(x)
-- ^ diag: missing-return
    if x then return 1 end
    NoReturnCrossfile_Warn("no value")
end

---@param s string?
local function narrowViaUntagged(s)
    if not s then NoReturnCrossfile_Warn("no value") end
    return s
    --     ^ hover: (param) s: string?
end

-- Defined below their callers in this file: the workspace scan already knows
-- a class's methods and the namespace's functions, so these narrow too.
---@class NoReturnCrossfileModule
local Module = {}

---@param s string?
function Module:Require(s)
    if not s then self:Bail("no value") end
    return s
    --     ^ hover: (param) s: string
end

---@param s string?
local function narrowViaLaterNamespace(s)
    if not s then ns.LaterFail("no value") end
    return s
    --     ^ hover: (param) s: string
end

---@param msg string
---@noreturn
function Module:Bail(msg)
end

---@param msg string
---@noreturn
function ns.LaterFail(msg)
end
