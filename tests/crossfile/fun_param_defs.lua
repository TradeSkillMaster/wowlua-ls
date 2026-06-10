---@diagnostic disable: unused-local, unused-function
local _, ns = ...

---@class FunParamDB
local FunParamDB = {}

---@return string?
function FunParamDB:GetVal(key) return nil end

---@class FunParamComm
local FunParamComm = {}

---@param cb fun(x: string): string
function FunParamComm.Load(cb) end

ns.FunParamComm = FunParamComm
