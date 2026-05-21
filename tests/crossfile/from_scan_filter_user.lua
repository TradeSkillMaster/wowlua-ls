-- Regression test: from_scan flag ensures workspace-scanned field assignments
-- (strings, numbers) are filtered by prescan.rs when the local class has
-- @field annotations, preserving inject-field diagnostics for undeclared fields.
-- inject-field only fires on @type instances, not @class definitions.

---@class FromScanFilterTest
---@field declared_name string
---@field declared_count number
local FromScanFilterTest = {}

---@type FromScanFilterTest
local obj = {}

-- Declared fields — no diagnostic
obj.declared_name = "test"
-- ^ diag: none
obj.declared_count = 1
-- ^ diag: none

-- Scanned fields from defs file — still inject-field because @field contract
-- means only declared fields are valid.
obj.scanned_label = "world"
--  ^ diag: inject-field
obj.scanned_value = 99
--  ^ diag: inject-field

-- Completely new field — inject-field
obj.brand_new = true
--  ^ diag: inject-field
