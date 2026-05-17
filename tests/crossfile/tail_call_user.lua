-- Cross-file tail-call return type test: usage
-- Verifies that cross-file callers of a tail-call wrapper function
-- see the concrete return types resolved from the same-file callee.

local addonName, ns = ...

-- Tail-call wrapper: concrete callee returns should propagate
local a, b = ns.Helpers.Fetch()
local _ = a
--        ^ hover: (local) a: string  def: local
local _ = b
--        ^ hover: (local) b: number  def: local
--        ^ diag: none
