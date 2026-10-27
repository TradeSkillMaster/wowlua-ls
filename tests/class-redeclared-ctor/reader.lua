-- Another file reads the declared fields, whichever declaration the workspace
-- build merges last.

---@param w RedeclWidget
local function read(w)
    local part = w.part
    --    ^ hover: (local) part: RedeclPart {
    local label = w.label
    --              ^ diag: access-protected
    return part, label
end

-- The `type` RedeclBase declares, through chains whose files are read in either
-- order (order_*.lua).
---@param first LeafFirst
---@param second LeafSecond
local function readTypes(first, second)
    return first.type, second.type
    --           ^ hover: (field) type: string
    --                        ^ hover: (field) type: string
end

return read, readTypes
