-- Test: @overload resolution

-- math.random has overloads:
--   fun():number
--   fun(m: integer):integer
--   primary: fun(m: integer, n: integer): integer

local a = math.random()        -- 0 args -> overload fun():number
--    ^ hover: (global) a: number  def: local
local b = math.random(10)      -- 1 arg  -> overload fun(m: integer):integer
--    ^ hover: (global) b: number  def: local

-- tonumber has overloads:
--   fun(e: string, base: integer):integer
--   primary: fun(e: any): number?

local d = tonumber("42")       -- 1 arg  -> primary: number?
--    ^ hover: (global) d: number  def: local
local e = tonumber("FF", 16)   -- 2 args -> overload: integer
--    ^ hover: (global) e: number  def: local

-- table.insert has overloads:
--   fun(list: table, value: any)
--   primary: fun(list: table, pos: integer, value: any)

local t = {}
table.insert(t, "hello")      -- 2 args -> overload (no return)
table.insert(t, 1, "hello")   -- 3 args -> primary (no return)

-- @overload on @class: callable table (e.g. LibStub)
-- LibStub is defined as @class with @overload fun(major: `T`, minor?: number): T, number?
---@class CallableTestLib
---@field Version number
local _CTL = {} ---@type CallableTestLib

local ctlib = LibStub("CallableTestLib")
--    ^ hover: (global) ctlib: CallableTestLib {
local ctver = ctlib.Version
--    ^ hover: (global) ctver: number
