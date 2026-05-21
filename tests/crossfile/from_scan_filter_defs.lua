-- Definitions file for from_scan filter regression test.
-- This file sets up a class with @field annotations AND assigns additional
-- fields (strings, numbers) from workspace scanning. The user file verifies
-- that inject-field still fires for undeclared fields — workspace-scanned
-- string/number assignments must not suppress inject-field diagnostics.

---@class FromScanFilterTest
---@field declared_name string
---@field declared_count number
local MyObj = {}

-- These assignments are discovered by workspace scanning (from_scan: true).
-- They should NOT suppress inject-field in the user file.
MyObj.scanned_label = "hello"
MyObj.scanned_value = 42
