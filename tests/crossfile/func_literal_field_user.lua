-- Cross-file test: function literal directly assigned to addon namespace field (usage)
---@diagnostic disable: unused-local
local private = select(2, ...)

-- Function literal returning a local function: the cross-file harvest lifts the
-- inner function losslessly into an inline `FunctionSig`, so the caller sees the
-- precise `fun(a: string)` signature (not a bare `function`).
local fn = private.Test()
--    ^ hover: (local) fn: fun(a: string)  def: local
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

-- Calling a FunctionSig: resolve_call_type extracts the return type from the
-- inline shape (not Any), and signature help shows the correct parameter list.
local greeter = private.GetGreeter()
--    ^ hover: (local) greeter: fun(name: string): string  def: local
local greeting = greeter("world")
--    ^ hover: (local) greeting: string  def: local
greeter("world")
--      ^ sig: fun(name: string): string

-- Function literal returning a local function on the non-early-exit path: the
-- cross-file harvest lifts the inner function losslessly into a `FunctionSig`.
-- The early-exit `return` contributes nil at slot 0, so the inferred type is the
-- optional, paren-wrapped `(fun(x: number))?`.
private.MaybeFunc(true)
--      ^ hover: (field) function MaybeFunc(flag)\n  -> (fun(x: number))?  def: external
