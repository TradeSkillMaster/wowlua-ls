---@meta

-- Test: `@meta` files run the annotation type-integrity passes beyond
-- undefined-doc-name (see tests/undefined-doc-name-meta.lua for that one). Each
-- structural/shape error below is a real mistake even in a declaration-only
-- stub, so it fires here; runtime/behavior diagnostics stay suppressed.

-- doc-field-no-class: a @field with no preceding @class/@enum declaration.
---@field orphanField number
-- ^ diag: doc-field-no-class

-- doc-func-no-function: a function-level tag not attached to a function.
---@return number
-- ^ diag: doc-func-no-function
local _notAFunc = 1

-- malformed-annotation: a @class with no name.
---@class
-- ^ diag: malformed-annotation

-- unknown-diag-code: a typo'd diagnostic code in a directive.
---@diagnostic disable-next-line: totally-not-a-real-code
-- ^ diag: unknown-diag-code
local _f = 1

-- nil-table-key: nil used as a table<K,V> key type in an annotation.
---@class NilKeyMeta : table<nil, number>
-- ^ diag: nil-table-key

-- A well-formed class + field fires nothing.
---@class GoodMeta
---@field ok number

-- Runtime/behavior diagnostics stay suppressed: this undefined global would fire
-- in a normal file but must not here.
local _unused = SomeUndefinedMetaGlobal2
