---@diagnostic disable: unused-local, unused-function
local _, ns = ...

---@type FunParamDB
local db = {}

---@type FunParamComm
local Comm = {}

-- Wrapper with inferred nullable return from cross-file method call
local function wrapper(x)
    return db:GetVal(x)
end
Comm.Load(wrapper)
--        ^ diag: type-mismatch

-- Wrapper with matching return type: no diagnostic
---@return string
local function alwaysStr() return "ok" end
local function goodWrapper(x) return alwaysStr() end
Comm.Load(goodWrapper)
