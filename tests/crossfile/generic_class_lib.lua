local _, ns = ...

---@class GenericReg<F>
---@field private _funcs table
local GenericReg = {}
ns.GenericReg = GenericReg

---@generic F
---@return GenericReg<F>
function GenericReg.New() return setmetatable({}, { __index = GenericReg }) end

---@param func F
function GenericReg:Register(func) end

---@param key string
---@param ... params<F>
---@return returns<F>
function GenericReg:Invoke(key, ...) end

---@param ... params<F>
function GenericReg:InvokeAll(...) end

---@class BaseItem
---@field id number
local BaseItem = {}
ns.BaseItem = BaseItem

---@class SpecialItem : BaseItem
---@field bonus number
local SpecialItem = {}
ns.SpecialItem = SpecialItem

---@alias ItemCallback fun(items: table<string, number>)

_G.useGenericClassLib = { GenericReg, BaseItem, SpecialItem }
