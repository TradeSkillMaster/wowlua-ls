-- Parent class whose defclass call AND constructor are in the same file.
-- The defclass scan discovers self._handle from the constructor body and
-- adds it to the parent's table as `any` (from the untyped `handle` param).
-- This field then propagates to child classes during workspace build.
local RPCtorParent = RPDefine("RPCtorParent")

function RPCtorParent:__init(handle)
    self._handle = handle  -- handle is untyped => _handle becomes any
end
