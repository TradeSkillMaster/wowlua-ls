---@diagnostic disable: create-global, undefined-global
-- Cross-file test: child class extends parent's field via self-referential assignment.
-- Verifies that method hover works on the chain despite the self-referential cycle.
-- This tests the single-level self-ref pattern: parent (SelfRefBaseWidget) has a
-- non-self-referential _SCHEMA, and this child reassigns it via the self-ref pattern.
local SelfRefDirectChild = DefineSelfRefWidget("SelfRefDirectChild", "SelfRefBaseWidget")
SelfRefDirectChild._SCHEMA = SelfRefDirectChild._SCHEMA:Extend("SelfRefDirectChildState")
    :AddStringField("title")
--   ^ hover: (method) function SelfRefSchema:AddStringField(key: string)
    :AddBoolField("active", false)
--   ^ hover: (method) function SelfRefSchema:AddBoolField(key: string, default: boolean)
    :Commit()
--   ^ hover: (method) function SelfRefSchema:Commit()
