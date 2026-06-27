---@diagnostic disable: unused-local, undefined-global
-- Cross-file caller for bare_local_field_defs.lua. The exhaustive diagnostic
-- harness fails on any unexpected diagnostic, so the *absence* of
-- `undefined-field`/`cannot-call`/`type-mismatch` on the reads below is itself
-- the assertion; the explicit `diag:` lines pin the negative control and the
-- deliberate `return-mismatch` tradeoff (see below).
---@class BareLocalFieldNS
local ns = select(2, ...)
local Util = ns.Util

-- The bare-local fields now resolve existence-only, typed `any`.
local lib = Util.Library
--               ^ hover: (field) Library: any
local c = Util.Computed
--             ^ hover: (field) Computed: any

-- `any` (not bare `table`) is deliberate (see build_on_stubs.rs). A *direct*
-- call must NOT false-positive as `cannot-call` — a bare `table` is not
-- callable, but `any` is. (A method call would NOT discriminate: field access
-- on a bare table yields `any`, which is callable, so it can't catch the
-- regression to `table` — only a direct call can.)
Util.Library()
-- ...and passing it where a string is expected must NOT `type-mismatch`
-- (a bare `table` would; `any` is permissive).
---@param s string
local function needsString(s) end
needsString(Util.Library)

-- The flip side of the `any` choice, asserted to lock the tradeoff in
-- deliberately: in the guarded-access return idiom `field and field.x`,
-- `any and any.x` resolves to `any?`, which isn't assignable to a non-nil
-- `@return`, so this over-reports `return-mismatch`. Bare `table` would make
-- this clean but reintroduce the cannot-call/type-mismatch above; measured
-- across real addons `any` is the lesser evil (this corner never lands on a
-- bare-local field in practice). If a future change types the field precisely
-- or makes `any and x` stay `any`, this diag disappears — update the test then.
---@return string
local function guardedReturn()
    return Util.Library and Util.Library.name
    --     ^ diag: return-mismatch
end

-- Negative control: a field assigned nowhere still reports `undefined-field`.
local bad = Util.NeverAssigned
--               ^ diag: undefined-field
