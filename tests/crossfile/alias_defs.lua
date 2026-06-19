-- Cross-file alias test: defines aliases consumed in alias_user.lua

---@alias XfCallback fun(x: number): boolean
---@alias XfResult string | number
---@alias XfStatus "ok" | "error" | "pending"

---@alias XfDict<K,V>: {[K]: V}

---@class XfProcessor
---@field name string

---@param cb XfCallback
---@return XfResult
function RunCallback(cb)
    return cb(1) and "ok" or 0
end

---@param status XfStatus
---@return boolean
function CheckStatus(status)
    return status == "ok"
end

-- Cross-file @return of a function-typed alias: the materialized signature must
-- survive into a caller's `local cb = GetXfCallback()` for type-checking.
---@return XfCallback
function GetXfCallback()
    ---@diagnostic disable-next-line: return-mismatch
    return function(x) return x > 0 end
end

-- Cross-file @param of a function-typed alias: callers passing non-function or
-- wrong-signature values must get type-mismatch.
---@param cb XfCallback
---@return boolean
function InvokeCallback(cb)
    return cb(42)
end

-- Cross-file @overload with function-typed alias param and return.
---@overload fun(cb: XfCallback): boolean
---@overload fun(x: number): number
function OverloadWithAlias(a)
    return a
end
