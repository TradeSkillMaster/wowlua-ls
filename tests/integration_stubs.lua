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

-- Compat globals (local alias → field ref, e.g. `local str = string; strmatch = str.match`)
local a = strmatch("hello", "(%w+)")
--        ^ hover: strmatch: fun(s: string | number, pattern: string | number, init?: number)  def: external

local b = strlen("hi")
--        ^ hover: strlen: fun(s: string | number): number  def: external

local c = tinsert
--        ^ hover: tinsert: fun(list: table, pos: number, value)  def: external

local d = floor(3.14)
--        ^ hover: floor: fun(x: number): number  def: external

local e = strsub("hello", 1, 3)
--        ^ hover: strsub: fun(s: string | number, i: number, j?: number): string  def: external
