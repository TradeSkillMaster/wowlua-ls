---@diagnostic disable: unused-local
-- Cross-file test: colon-call to dot-defined methods should not produce missing-param
local addonName, ns = ...
local DCC = ns.DCC

-- Colon call to dot-defined static method — cls is implicitly passed
DCC:_ExtendStateSchema()
--  ^ hover: (method) function DotColonClass:_ExtendStateSchema(cls)

-- Colon call with one explicit arg (cls is implicit)
DCC:_AddActionScripts("OnShow")

-- Colon call to varargs dot-defined method — multiple args should be fine
DCC:_AddMultipleScripts("OnShowContents", "OnStartOpening")

-- Colon call to method with unannotated param — no missing-parameter warning
DCC:_CreateFrame()
