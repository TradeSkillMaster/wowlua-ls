---@diagnostic disable: create-global
-- Scanned before `defs.lua`, which declares `PosKinds.Mode` an `@enum`: the
-- declaration types the table, not this constructor's shape.
PosKinds = {}
PosKinds.Mode = { Off = 0, On = 1 }
