---@diagnostic disable: create-global, unused-local
-- Cross-file regression test: assigning to optional built-table fields
-- should NOT trigger field-type-mismatch when @param type is T|`T`
-- (Bug #15: built-table optional fields lose non-nil type)
local Component = DefineClass("ChainTestComponent")
local BNBuilder = Component:Include("BNBuilder")

local STATE = BNBuilder.CreateSchema("ASSIGN_TEST_STATE")
    :AddOptionalClassField("item", "BNFieldBase")
    :AddStringField("name")
    :Commit()

---@param state ASSIGN_TEST_STATE
function testAssignOptionalField(state)
    ---@type BNFieldBase
    local myItem = {}
    state.item = myItem
    --           ^ hover: (local) myItem: BNFieldBase
end
