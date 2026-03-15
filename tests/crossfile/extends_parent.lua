-- Cross-file @built-extends test: parent class with @built-name schema field
-- Tests that when a child overrides a parent's @built-name field via expression
-- statement, the inherited constructor field types get substituted.

local ParentElem = DefineClassWithParent("ExtendsParentElem")

-- Static field: BNReactive.CreateSchema is a global function with @built-name propagation.
-- Creates a named built type "ParentElemState" with fields baseName:string.
ParentElem._SCHEMA = BNReactive.CreateSchema("ParentElemState"):AddStringField("baseName"):Lock()

-- Constructor sets self._state from the schema
function ParentElem:__init()
    self._state = self._SCHEMA:CreateState()
end
