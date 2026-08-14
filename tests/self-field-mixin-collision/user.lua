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
-- The write is `self.nested = 1`, so the cross-file `@class` field-type harvester
-- upgrades the coarse `any` to the definition-site type `number` (deferred.rs) —
-- the field's existence and misattribution guarantees above are unaffected.
---@type NestedClass
local widget = nil
local n = widget.nested
--               ^ hover: (field) nested: number
