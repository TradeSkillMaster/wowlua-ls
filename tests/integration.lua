-- wow_ls integration test
-- Annotations on the line below code use caret to mark test column
-- Format: --  caret hover: TYPE  def: local|external|None

local x = 5
--    ^ hover: x: number  def: local

local y = x + 2
--    ^ hover: y: number  def: local

local s = "hello"
--    ^ hover: s: string  def: local

local b = true
--    ^ hover: b: true  def: local

local n = nil
--    ^ hover: n: nil  def: local

local function AddTwo(val)
    return val + 2
end

local result = AddTwo(x)
--    ^ hover: result: number  def: local

local f = AddTwo
--    ^ hover: f: fun(val: number): number  def: local

local function GetPair()
    return 11, 22
end
local a, b2 = GetPair()
--    ^ hover: a: number  def: local

do
    local inner = 99
    --    ^ hover: inner: number  def: local
    local sum = inner + x
    --    ^ hover: sum: number  def: local
end

-- WoW addon varargs: local addonName, ns = ...
local addonName, ns = ...
--    ^ hover: addonName: string  def: local
ns.version = 1
ns.title = "MyAddon"
local ver = ns.version
--    ^ hover: ver: number  def: local
local title = ns.title
--    ^ hover: title: string  def: local
