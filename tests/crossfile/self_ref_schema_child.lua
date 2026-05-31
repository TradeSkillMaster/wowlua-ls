---@diagnostic disable: create-global, undefined-global
-- Cross-file test: child class extends parent's field via self-referential assignment.
-- Pattern: ChildClass.field = ChildClass.field:Extend("NewName"):...:Commit()
local SelfRefChildPanel = DefineSelfRefWidget("SelfRefChildPanel", "SelfRefBaseWidget")
SelfRefChildPanel._SCHEMA = SelfRefChildPanel._SCHEMA:Extend("SelfRefChildPanelState")
    :AddBoolField("visible", true)
    :Commit()
