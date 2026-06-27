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

-- forwarded-value fields are callable-or-unknown (`function & table`): calling
-- them must NOT false-positive as `cannot-call` (the exhaustive harness fails on
-- any unexpected diagnostic, so the absence of `cannot-call` on these calls is
-- itself the assertion).
addonTable.OnClick("click")          -- forwarded from a parameter
--         ^ hover: (field) OnClick: function & table
local gv = addonTable.GetValue()     -- forwarded from another field
--                    ^ hover: (field) GetValue: function & table
addonTable.Sub.Run()                 -- forwarded onto a deep path
-- a function-literal field stays plain `function` (not the forwarded intersection)
addonTable.Render()
--         ^ hover: (field) Render: function

-- negative control: a field assigned nowhere still reports undefined-field
local bad = addonTable.NeverAssigned
--                     ^ diag: undefined-field
