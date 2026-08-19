-- Cross-file sibling-method test: definitions
-- A frame instance here defines a method whose name also appears on a different
-- frame instance in the user file. Both frames share the external `Frame` class,
-- so this file's method registers on it cross-file — but the user file's own
-- definition must win for its own frame, not leak this sibling's signature.
-- Requires: --with-stubs

local siblingFrame = CreateFrame("Frame")
--- @param count number
function siblingFrame:XfileSharedMethod(count) end
