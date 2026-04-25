local _, ns = ...

---@class XPool<T>
local XPool = {}
ns.XPool = XPool

---@generic T
---@param cls T
---@return XPool<T>
function XPool.New(cls) end

---@param obj T
function XPool:Recycle(obj) end

---@class XAnimal

---@class XCat : XAnimal
local XCat = {}
ns.XCat = XCat

---@param task XAnimal
function ns.RemoveTask(task) end

_G.usePoolLib = { XPool, XCat }
