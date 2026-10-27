local _, ns = ... ---@class ClassCtorFieldsNS

-- The workspace build types a `@class` constructor's call-valued entries from the
-- file that writes them (class_ctor_fields_defs.lua); the cold build leaves `any`.
local CallCtorFields = ns.CallCtorFields
local _call1 = CallCtorFields.FromCall
--                            ^ hover: (field) FromCall: number
local _call2 = CallCtorFields.FromMethod
--                            ^ hover: (field) FromMethod: string
