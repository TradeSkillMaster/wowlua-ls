-- wow_ls integration test (with stubs)
-- Requires: --with-stubs

local t = setmetatable({}, {})
--        ^ hover: setmetatable: fun(tbl: table, metatable?: metatable | table): table  def: external

local s = type("hello")
--        ^ hover: type: fun(v)  def: external

local ok = pcall(print, "hi")
--         ^ hover: pcall: fun(f: function, arg1?, ...): boolean  def: external

---@type Frame
local f = nil
--    ^ hover: f: Frame  def: local
