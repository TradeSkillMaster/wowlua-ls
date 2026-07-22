-- Regression: a class/library field assigned from a *chained* generic-getter
-- call must resolve to the outer method's return, not the inner getter's
-- receiver class.
--
-- `LibStub("CallbackHandler-1.0"):New(...)` returns a `CallbackHandlerRegistry`;
-- the receiver `LibStub("CallbackHandler-1.0")` is the library `CallbackHandler-1.0`
-- (which has no `Fire`). The coarse workspace field scan resolves this idiom via
-- the `[CallbackHandler-1.0, New]` chain (receiver string = class, then method),
-- so the field is the registry `New` returns — never the library named by the
-- inner string arg (that arg is NOT used as the field type). Previously the scan
-- fell back to that inner string ("CallbackHandler-1.0") and typed the field as the
-- library, so `field:Fire()` false-positived
-- `undefined-field 'Fire' on class 'CallbackHandler-1.0'`.
---@diagnostic disable: unused-local

-- Real-world defensive-init form (`field = field or getter():New(self, ...)`),
-- as Ace-style callback libraries write it. Must not false-positive on `:Fire`.
local lib = LibStub:NewLibrary("MyCallbackLib-1.0", 1)
lib.callbacks = lib.callbacks or LibStub("CallbackHandler-1.0"):New(lib, "RegisterCallback")

function lib:Announce()
    lib.callbacks:Fire("MyEvent")
end

-- Direct (non-`or`) field form: the coarse scan recovers the precise registry type
-- via the `[CallbackHandler-1.0, New]` chain, so `:Fire` resolves to the registry
-- method (not the library `CallbackHandler-1.0`).
local other = LibStub:NewLibrary("MyOtherLib-1.0", 1)
other.cbs = LibStub("CallbackHandler-1.0"):New(other)
other.cbs:Fire("X")
--        ^ hover: (method) function CallbackHandlerRegistry:Fire(eventname: string, ...: unknown)
