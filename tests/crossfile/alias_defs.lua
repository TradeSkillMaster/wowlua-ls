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
