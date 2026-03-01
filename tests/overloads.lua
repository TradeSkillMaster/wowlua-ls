-- Test: @overload resolution

-- math.random has overloads:
--   fun():number
--   fun(m: integer):integer
--   primary: fun(m: integer, n: integer): integer

local a = math.random()        -- 0 args -> overload fun():number
local b = math.random(10)      -- 1 arg  -> overload fun(m: integer):integer
local c = math.random(1, 10)   -- 2 args -> primary fun(m, n): integer

-- tonumber has overloads:
--   fun(e: string, base: integer):integer
--   primary: fun(e: any): number?

local d = tonumber("42")       -- 1 arg  -> primary: number?
local e = tonumber("FF", 16)   -- 2 args -> overload: integer

-- table.insert has overloads:
--   fun(list: table, value: any)
--   primary: fun(list: table, pos: integer, value: any)

local t = {}
table.insert(t, "hello")      -- 2 args -> overload (no return)
table.insert(t, 1, "hello")   -- 3 args -> primary (no return)
