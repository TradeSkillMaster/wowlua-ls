-- A `@defclass` class whose constructor stores a generic call's result: the
-- scan types the field with the call's `@return XFieldStream<T>`, whose `T` only
-- the call binds.

---@class XFieldStream<T>
local XFieldStream = {}

---@param data T
function XFieldStream:Send(data) end

---@generic T
---@param fn fun(): T
---@return XFieldStream<T>
function GetXFieldStream(fn) return fn and XFieldStream end

---@class XFieldObjBase
---@constructor __init

---@generic T: XFieldObjBase
---@defclass T
---@param name `T`
---@return T
function DefineXFieldClass(name)
    return {}
end
