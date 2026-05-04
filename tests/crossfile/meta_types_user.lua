-- Uses types defined in meta_types.lua (@meta file).
-- Should not produce undefined-doc-name warnings.

---@param frames MetaFrameTable
---@return number
local function countFrames(frames)
    local n = 0
    for _ in pairs(frames) do n = n + 1 end
    return n
end
countFrames({})

---@type MetaFrameData
local data = { x = 1, y = 2 }
--    ^ hover: (local) data: MetaFrameData  def: local  diag: unused-local
