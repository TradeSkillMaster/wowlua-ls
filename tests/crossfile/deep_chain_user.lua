-- Deep cross-file test: consumes 4+ part chains defined in deep_chain_defs.lua
local _, addon = ...

-- Leaf field hover: type propagates through auto-created intermediates.
-- Intermediates (Data, Config) and the leaf (version) all resolve across files.
local v = addon.Data.Config.version
--    ^ hover: (global) v: number  def: local
--              ^ def: external
--                   ^ def: external
--                          ^ def: external
local t = addon.Data.Config.title
--    ^ hover: (global) t: string  def: local
--              ^ def: external
--                          ^ def: external

-- Intermediate-only access: hover shows a sub-table
local cfg = addon.Data.Config
--    ^ hover: (global) cfg: {  def: local

-- 5-part chain: leaf field
local count = addon.Deep.Nested.Inner.Leaf.count
--    ^ hover: (global) count: number  def: local

-- 4-part method call
local s = addon.Engine.Core:Start()
--    ^ hover: (global) s: string  def: local
--                          ^ hover: (method) function Start()  def: external

-- 5-part method call
local n = addon.Engine.Core.Parser:Parse()
--    ^ hover: (global) n: number  def: local
--                                 ^ hover: (method) function Parse()  def: external

-- Negative test: deep writes on a non-addon-ns root (Alien.Ship.Engine.Fuel)
-- in deep_chain_nonroot.lua must not fabricate sub-tables on Alien.
local ship = Alien.Ship
--                 ^ diag: undefined-field

-- Type conflict: ns.Conflict is a string in deep_chain_defs.lua, so the deep
-- write `ns.Conflict.shouldNotExist = 42` must not overwrite it with a table.
local conflict = addon.Conflict
--    ^ hover: (global) conflict: string  def: local

-- Deep methods on a buffered local table, flushed via `ns.Db = LocalDb`.
-- Both the direct method and the deeper ones resolve cross-file.
local d = addon.Db:Direct()
--    ^ hover: (global) d: string  def: local
local one = addon.Db.Sub:OneDeep()
--    ^ hover: (global) one: string  def: local
local two = addon.Db.Sub.Inner:TwoDeep()
--    ^ hover: (global) two: string  def: local
