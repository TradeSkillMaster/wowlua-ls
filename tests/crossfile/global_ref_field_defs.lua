-- Cross-file test: stub function assigned to table field

DebugUtil = {}

-- Assign stub global functions to table fields
DebugUtil.Stack = debugstack
DebugUtil.Locals = debuglocals
