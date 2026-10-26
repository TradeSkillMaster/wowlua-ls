-- Adding to a `@class` field list another file builds (`holder.lua`) stores an
-- element: the value isn't checked against the elements that file stores.
---@diagnostic disable: unused-local, unused-function

---@param holder CrossHolder
---@param count number
local function addCount(holder, count)
    tinsert(holder.entries, count)
    --             ^ hover: (field) entries: CrossItem[]
    holder.entries[#holder.entries + 1] = count
end
