---@diagnostic disable: unused-local, undefined-global
-- Cross-file caller: reading namespace fields that were assigned only inside
-- function bodies in infunc_field_defs.lua must not false-positive as
-- `undefined-field`. The exhaustive diagnostic harness fails on any unexpected
-- diagnostic, so the absence of `undefined-field` on the valid reads below is
-- itself an assertion; the explicit `diag: undefined-field` on a never-assigned
-- field proves the class is closed (i.e. the test isn't vacuously passing).
---@class InFuncNS
local addonTable = select(2, ...)

-- single-target in-function field (existence-only: typed as a bare `table`)
local t = addonTable.Title
--                   ^ hover: (field) Title: table
-- multi-target in-function fields
local w = addonTable.Width
local h = addonTable.Height
-- deep-chain in-function field (both the sub-table and its field)
local v = addonTable.Sub.Value
-- complex in-function field: a method access on it must not false-positive
addonTable.InFuncWidget:Ping()

-- top-level multi-target fields (outside any function body)
local mx = addonTable.MinX
local mxx = addonTable.MaxX

-- negative control: a field assigned nowhere still reports undefined-field
local bad = addonTable.NeverAssigned
--                     ^ diag: undefined-field
