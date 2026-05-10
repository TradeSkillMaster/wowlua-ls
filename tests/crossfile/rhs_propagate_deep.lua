-- Deep hierarchy: RPElement → RPContainer → RPBaseFrame
-- Class definitions are in rhs_propagate_deep_defs.lua (separate file).
-- This file only defines methods on the already-existing external classes,
-- simulating the TSM pattern where class methods live in different files.

function RPElement:__init(frame)
    self._frame = frame  -- frame is untyped => _frame is any
end

function RPContainer:__init()
    -- Container doesn't override _frame
end

function RPBaseFrame:__init()
    local frame = RPCreateWidget()
    self._frame = frame  -- concrete type assignment
end

function RPBaseFrame:DoWork()
    -- _frame should be RPWidget (from grandchild's assignment), not any
    local f = self._frame
    --    ^ hover: (local) f: RPWidget {
end
