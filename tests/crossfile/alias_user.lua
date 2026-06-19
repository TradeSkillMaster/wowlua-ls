---@diagnostic disable: unused-function, unused-local
-- Cross-file alias test: uses aliases defined in alias_defs.lua

-- Union alias in @type
---@type XfResult
local res = "hello"
--    ^ hover: (local) res: string | number  def: local

-- String literal union alias (order matches alias declaration)
---@type XfStatus
local st = "ok"
--    ^ hover: (local) st: "ok" | "error" | "pending"  def: local

-- Cross-file function using alias param type
local val = RunCallback(function(x) return true end)
--    ^ hover: (local) val: string | number  def: local

-- Cross-file function using alias param type
local ok = CheckStatus("ok")
--     ^ hover: (local) ok: boolean  def: local

-- Alias used locally in @param
---@param cb XfCallback
---@return boolean
local function runIt(cb)
    return cb(5)
end

-- Function-type alias in @type is propagated through assignments: `h` inherits
-- the full fun(...) signature from `handler`, not a collapsed `function` type.
---@type XfCallback
local handler
local h = handler
--    ^ hover: (local) function h(x: number)\n-> boolean  def: local

-- Cross-file @return of a function-typed alias yields the full signature, so the
-- returned callback gets argument type-checking.
local xfcb = GetXfCallback()
--    ^ hover: (local) function xfcb(x: number)\n-> boolean  def: local
xfcb("bad")
-- ^ diag: type-mismatch

-- Cross-file @param with function-typed alias: wrong type triggers type-mismatch.
InvokeCallback("not a function")
--             ^ diag: type-mismatch
InvokeCallback(function(x) return true end)

-- Cross-file @overload with function-typed alias param: wrong type triggers mismatch.
local ov1 = OverloadWithAlias(function(x) return true end)
--    ^ hover: (local) ov1: boolean  def: local
local ov2 = OverloadWithAlias(10)
--    ^ hover: (local) ov2: number  def: local
