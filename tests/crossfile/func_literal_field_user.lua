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

-- Function literal with body-derived return: the cross-file caller sees the
-- precise inferred return (`"Hello, " .. name` → string), not coarse `any`.
private.MakeGreeting("world")
--      ^ hover: (field) function MakeGreeting(name)\n  -> string  def: external

-- Function literal returning a local function on the non-early-exit path: the
-- cross-file return lifts to a bare `function` (the inner signature can't be
-- referenced cross-file). The early-exit `return` contributes no slot-0 value,
-- so the inferred type is `function`, an upgrade over coarse `any`.
private.MaybeFunc(true)
--      ^ hover: (field) function MaybeFunc(flag)\n  -> function  def: external
