---@diagnostic disable: unused-local
-- Cross-file @built-extends test: child overrides parent's @built-name schema field
-- Tests field_built_names substitution in PreResolvedGlobals inheritance.

local ParentElem = DefineClassWithParent("ExtendsParentElem")
local ChildElem = DefineClassWithParent("ExtendsChildElem", ParentElem)

-- Expression statement: extends the parent's _SCHEMA with a new @built-name
ChildElem._SCHEMA:Extend("ChildElemState"):AddStringField("childField"):Lock()

-- ChildElem inherits _state from ExtendsParentElem.__init.
-- The type should be substituted from ParentElemState to ChildElemState.
local child = DefineClassWithParent("ExtendsChildElem")
local child_state = child._state
--    ^ hover: (local) child_state: ChildElemState

-- Inherited field from ParentElemState should be accessible through parent_classes
local child_base = child._state.baseName
--    ^ hover: (local) child_base: string
