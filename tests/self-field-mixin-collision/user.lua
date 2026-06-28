---@diagnostic disable: unused-local
-- The deeply-nested method's `self.deep` must NOT have leaked onto the unrelated
-- top-level `Collision` (the misattribution bug), while the single-name
-- `PlainMixin`'s `self.shallow` must still resolve.
local leaked = Collision.deep
--                       ^ hover: <missing>
local ok = PlainMixin.shallow
--                    ^ hover: (field) shallow: any
-- The deeply-nested @class `Outer.Sub.Widget`'s control-flow-nested `self.nested`
-- write must resolve on a NestedClass-typed value (re-keyed via var_to_class).
-- Existence-only fields register as `any` (the honest "unknown"), not a bare
-- `table` that would leak into reads as a false `type-mismatch`/`cannot-call`.
---@type NestedClass
local widget = nil
local n = widget.nested
--               ^ hover: (field) nested: any
