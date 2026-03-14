-- Cross-file test: colon-call to dot-defined methods should not produce missing-param
local addonName, ns = ...
local DCC = ns.DCC

-- Colon call to dot-defined static method — cls is implicitly passed
DCC:_ExtendStateSchema()
--  ^ diag: none

-- Colon call with one explicit arg (cls is implicit)
DCC:_AddActionScripts("OnShow")
--  ^ diag: none

-- Colon call to varargs dot-defined method — multiple args should be fine
DCC:_AddMultipleScripts("OnShowContents", "OnStartOpening")
--  ^ diag: none

-- Colon call to method with unannotated param — no missing-parameter warning
DCC:_CreateFrame()
--  ^ diag: none
