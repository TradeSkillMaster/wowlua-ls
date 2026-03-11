-- Cross-file test: colon-call to dot-defined methods should not produce missing-param
local addonName, ns = ...
local DCC = ns.DCC

-- Colon call to dot-defined static method — cls is implicitly passed
DCC:_ExtendStateSchema()
--  ^ diag: none

-- Colon call with one explicit arg (cls is implicit)
DCC:_AddActionScripts("OnShow")
--  ^ diag: none
