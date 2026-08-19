-- Cross-file sibling-method test: usage
-- Requires: --with-stubs

local userFrame = CreateFrame("Frame")
--- @param label string
function userFrame:XfileSharedMethod(label) end

-- Direct hover resolves this file's own definition (label: string), not the
-- sibling's (count: number).
userFrame:XfileSharedMethod("ok")
--        ^ hover: (method) function Frame:XfileSharedMethod(label: string)

-- The fixpoint-resolved arg check also uses this frame's own signature: passing a
-- number reports `label`, proving it did not bind to the sibling frame's
-- `count: number` method registered on the shared Frame class cross-file.
userFrame:XfileSharedMethod(42)
--        ^ diag: type-mismatch
