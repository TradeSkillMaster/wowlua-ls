-- Cross-file defclass subtype test: passes defclass-created class where parent is expected.
-- MY_ENUM is a subclass of EnumBase (created via @defclass T: EnumBase),
-- so passing it to a function expecting EnumBase should NOT produce type-mismatch.

-- Direct field access: type should be MY_ENUM (subclass of EnumBase)
local e = EnumStore.MY_ENUM
--    ^ hover: (local) e: MY_ENUM {

-- Passing defclass-created class to function expecting parent class: no type-mismatch
AcceptEnum(EnumStore.MY_ENUM, "scan")
-- ^ diag: none

-- Also test with a local variable
local myEnum = EnumStore.MY_ENUM
AcceptEnum(myEnum, "scan")
-- ^ diag: none

-- Inherited field access should work
local v = EnumStore.MY_ENUM.value
--    ^ hover: (local) v: number
