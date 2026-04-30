-- Cross-file nested enum test: uses NewNested with nested table literals
local MY_ENUM = XEnumNewNested("MY_ENUM", {
    GROUP_A = {
        ITEM_1 = XEnumNewValue(),
        ITEM_2 = XEnumNewValue(),
    },
    GROUP_B = {
        ITEM_3 = XEnumNewValue(),
    },
    FLAT = XEnumNewValue(),
})

local grp = MY_ENUM.GROUP_A
--    ^ hover: (local) grp: {  def: local
local val = MY_ENUM.GROUP_A.ITEM_1
--    ^ hover: (local) val: XEnumValue  def: local
local val2 = MY_ENUM.GROUP_B.ITEM_3
--    ^ hover: (local) val2: XEnumValue  def: local
local flat = MY_ENUM.FLAT
--    ^ hover: (local) flat: XEnumValue  def: local

-- Defclass enum should be assignable to parent class XEnumObject
XEnumIsType(MY_ENUM)
-- ^ diag: none

-- Deep nested enum (3+ levels cross-file)
local DEEP_ENUM = XEnumNewNested("DEEP_ENUM", {
    CATEGORY = {
        SUB_CAT = {
            LEAF_A = XEnumNewValue(),
            LEAF_B = XEnumNewValue(),
        },
        DIRECT = XEnumNewValue(),
    },
})

local deepA = DEEP_ENUM.CATEGORY.SUB_CAT.LEAF_A
--    ^ hover: (local) deepA: XEnumValue  def: local
local deepB = DEEP_ENUM.CATEGORY.SUB_CAT.LEAF_B
--    ^ hover: (local) deepB: XEnumValue  def: local
local deepDirect = DEEP_ENUM.CATEGORY.DIRECT
--    ^ hover: (local) deepDirect: XEnumValue  def: local

-- Cross-table field reference: alias a defclass enum through another table
local Wrapper = {}
Wrapper.ENUM = MY_ENUM
local ref = Wrapper.ENUM
--    ^ hover: (local) ref: MY_ENUM  def: local
XEnumIsType(Wrapper.ENUM)
-- ^ diag: none
