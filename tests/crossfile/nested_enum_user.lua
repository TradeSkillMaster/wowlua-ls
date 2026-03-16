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
--    ^ hover: (global) grp: {  def: local
local val = MY_ENUM.GROUP_A.ITEM_1
--    ^ hover: (global) val: XEnumValue  def: local
local val2 = MY_ENUM.GROUP_B.ITEM_3
--    ^ hover: (global) val2: XEnumValue  def: local
local flat = MY_ENUM.FLAT
--    ^ hover: (global) flat: XEnumValue  def: local
