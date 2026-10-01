-- `self` in a @field type names the declaring class when read from another file.

local cloned = SelfTypeFieldData.clone()
--    ^ hover: (local) cloned: SelfTypeFieldData
SelfTypeFieldData.onUpdate(cloned)
SelfTypeFieldData.onUpdate(1)
--                         ^ diag: type-mismatch
