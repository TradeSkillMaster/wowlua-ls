-- Regression: a class/library field assigned from a *chained* generic-getter
-- call must resolve to the outer method's return, not the inner getter's
-- receiver class.
--
-- `LibStub("CallbackHandler-1.0"):New(...)` returns a `CallbackHandlerRegistry`;
-- the receiver `LibStub("CallbackHandler-1.0")` is the library `CallbackHandler-1.0`
-- (which has no `Fire`). The coarse workspace field scan cannot resolve the
-- transforming outer `:New()`, and previously fell back to the inner string arg
-- ("CallbackHandler-1.0") to type the field as that library — so `field:Fire()`
-- false-positived `undefined-field 'Fire' on class 'CallbackHandler-1.0'`.
-- Dropping the inner string arg for chained receivers lets the per-file fixpoint
-- resolve the real return type (or leave it a permissive `table`/`any`).
---@diagnostic disable: unused-local

-- Real-world defensive-init form (`field = field or getter():New(self, ...)`),
-- as Ace-style callback libraries write it. Must not false-positive on `:Fire`.
local lib = LibStub:NewLibrary("MyCallbackLib-1.0", 1)
lib.callbacks = lib.callbacks or LibStub("CallbackHandler-1.0"):New(lib, "RegisterCallback")

function lib:Announce()
    lib.callbacks:Fire("MyEvent")
end

-- Direct (non-`or`) field form: the fixpoint recovers the precise registry type,
-- so `:Fire` resolves to the registry method (not the library `CallbackHandler-1.0`).
local other = LibStub:NewLibrary("MyOtherLib-1.0", 1)
other.cbs = LibStub("CallbackHandler-1.0"):New(other)
other.cbs:Fire("X")
--        ^ hover: (method) function CallbackHandlerRegistry:Fire(eventname: string, ...: unknown)
