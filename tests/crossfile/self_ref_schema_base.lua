---@diagnostic disable: create-global, undefined-global
-- Cross-file test: base class sets field from initial creation
local BaseWidget = DefineSelfRefWidget("SelfRefBaseWidget")
BaseWidget._SCHEMA = SelfRefSchema.Create("SelfRefBaseWidgetState")
    :AddStringField("id")
    :Commit()
