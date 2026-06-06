-- Cross-file test: function literal directly assigned to addon namespace field (usage)
---@diagnostic disable: unused-local
local private = select(2, ...)

-- Function literal returning an annotated inner function should preserve its type
local fn = private.Test()
--    ^ hover: (local) function fn(a: string)  def: local
private.Test()
--      ^ hover: (field) function Test()\n  -> fun(a: string)  def: external

-- Function literal with @param/@return annotations should preserve full signature
local sum = private.Add(1, 2)
--    ^ hover: (local) sum: number  def: local
private.Add(1, 2)
--      ^ hover: (field) function Add(x: number, y: number)\n  -> number  def: external

-- Function literal with body-derived return
private.MakeGreeting("world")
--      ^ hover: (field) function MakeGreeting(name)\n  -> any  def: external

-- Function literal with bare return: return type stays `any` (not fun(x: number))
-- because the function can return nil on the early-exit path
private.MaybeFunc(true)
--      ^ hover: (field) function MaybeFunc(flag)\n  -> any  def: external
