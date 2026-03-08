-- Cross-file chain test: uses Include to get a class from another file,
-- then exercises method chains with @return self and resolves the final type.
-- Tests: auto-created class tables from pre_globals + external expr cycle detection.
local Component = DefineClass("ChainTestComponent")
local Schema = Component:Include("ChainSchema")
--     ^ hover: Schema: ChainSchema

-- Long method chain with repeated @return self calls.
-- This tests that external expr cycle detection doesn't break the chain.
local db = Schema:AddField("name"):AddNumberField("count"):AddField("label"):Commit()
--    ^ hover: db: ChainSchemaResult

-- Method on the result of the chain should resolve
db.Query()
-- ^ diag: none

-- Chain via From():Include() (3-part chain)
local Schema2 = Component:From("ChainTestComponent"):Include("ChainSchema")
--     ^ hover: Schema2: ChainSchema  diag: unused-local
