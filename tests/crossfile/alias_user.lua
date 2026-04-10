-- Cross-file alias test: uses aliases defined in alias_defs.lua

-- Union alias in @type
---@type XfResult
local res = "hello"
--    ^ hover: (global) res: string | number  def: local

-- String literal union alias (order matches alias declaration)
---@type XfStatus
local st = "ok"
--    ^ hover: (global) st: "ok" | "error" | "pending"  def: local

-- Cross-file function using alias param type
local val = RunCallback(function(x) return true end)
--    ^ hover: (global) val: string | number  def: local

-- Cross-file function using alias param type
local ok = CheckStatus("ok")
--     ^ hover: (global) ok: boolean  def: local

-- Alias used locally in @param
---@param cb XfCallback
---@return boolean
local function runIt(cb)
    return cb(5)
end

-- Function-type alias in @type applied to non-function variable
-- NOTE: The function alias resolves to generic "function" type on propagation.
-- This is a known limitation — function type aliases lose signature details
-- when propagated through assignment (see gap summary).
---@type XfCallback
local handler
local h = handler
--    ^ hover: (global) h: function  def: local
