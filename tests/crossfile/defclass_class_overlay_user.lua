-- Regression: cross-file access to a defclass class with @class overlay.
-- The constructor must still be recognized via DefineClass.

local inst = DefineClass("ReactiveOneShot")()
--    ^ hover: (local) inst: ReactiveOneShot

-- Constructor field from __init must be visible cross-file
local v = inst._value
--             ^ hover: (field) _value: any!  diag: unused-local
