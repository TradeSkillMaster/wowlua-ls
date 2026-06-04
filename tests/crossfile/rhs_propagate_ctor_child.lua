---@diagnostic disable: undefined-global, unused-local
-- Child class (3-level deep) whose parent's any-typed _handle field is
-- overridden with a concrete type. The parent field was discovered by the
-- defclass constructor scan and propagated to the child's own table, so the
-- overlay inherits the parent's Any annotation. The fix ensures the
-- expression-based resolution path is used instead of the inherited Any.
local RPCtorMiddle = RPDefine("RPCtorMiddle", RPCtorParent)
local RPCtorChild = RPDefine("RPCtorChild", RPCtorMiddle)

function RPCtorChild:__init()
    local w = RPCreateWidget()
    self._handle = w  -- override parent's any with concrete RPWidget
end

function RPCtorChild:DoWork()
    -- _handle should be RPWidget (from child's assignment), not any
    local h = self._handle
    --    ^ hover: (local) h: RPWidget {
end
