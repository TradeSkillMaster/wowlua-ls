---@meta _
-- A `---@meta` declaration file: everything here is a type-declaration stub
-- (the workspace analogue of the built-in WoW API stubs). "No references in
-- the workspace" is the expected state for a stub, so NONE of these functions
-- may be reported as unused-function.

---@class MetaLib
local lib = {}

-- Method on a local `@class`-typed table (the exact shape from the bug report:
-- `local lib` + `@class` + `function lib:Method()`). Registered as the external
-- method `MetaLib:GetRegion` but must be excluded from the cross-file check.
---@return string?
function lib:GetRegion() end

---@return number
function lib:GetCount() end

-- A top-level global function declared in the meta file — also a declaration
-- stub, so it exercises the Pass-1 (global) exclusion too.
---@return boolean
function MetaGlobalStub() end
