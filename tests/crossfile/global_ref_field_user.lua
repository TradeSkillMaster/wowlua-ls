-- Cross-file test: calling stub function through table field

-- Calling the aliased stub function should not trigger cannot-call
local stack = DebugUtil.Stack(2, 1, 0)
--    ^ hover: (local) stack: string  def: local

local _ = stack
