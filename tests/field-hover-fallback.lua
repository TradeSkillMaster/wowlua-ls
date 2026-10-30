---@diagnostic disable: unused-local, undefined-global
-- A field name the hover finds no member for still hovers: the type the read
-- resolves to (`any` through an `any` receiver), else `?`.
local unknown = MissingGlobal
local a = unknown.field
--                ^ hover: (field) field: ?
unknown:Method()
--      ^ hover: (method) Method: ?

---@type any
local anything
local b = anything.field
--                 ^ hover: (field) field: any
anything:Method()
--       ^ hover: (method) Method: any

_G.useFieldHoverFallback = { a, b }
