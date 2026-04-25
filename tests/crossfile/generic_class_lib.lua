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

---@class CallableIter<F>
---@overload fun(): returns<F>
local CallableIter = {}
ns.CallableIter = CallableIter

---@return CallableIter<fun(): number, string>
function ns.MakeConcreteIter() return {} end

---@return CallableIter<fun(): number, ...>
function ns.MakeVarargIter() return {} end

---@return CallableIter<fun(): number, ...string>
function ns.MakeTypedVarargIter() return {} end

---@class Container<F>
---@field private _iter CallableIter<F>
local Container = {}
ns.Container = Container

---@return CallableIter<F>
function Container:GetIterator() return self._iter end

---@class QueryBuilder
local QueryBuilder = {}
ns.QueryBuilder = QueryBuilder

---@param name string
---@return QueryBuilder
function QueryBuilder:Filter(name) return self end

---@return CallableIter<fun(): number, string>
function QueryBuilder:Iterator() return {} end

_G.useGenericClassLib = { GenericReg, BaseItem, SpecialItem, CallableIter, Container, QueryBuilder }
