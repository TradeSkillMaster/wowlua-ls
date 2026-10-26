function PosTopOfFileName(name)
    return name .. "!"
end
-- A function at the very top of its file gives other files its own returns, not
-- those of an annotation's `fun(...)`, which starts at offset 0 too.
---@param cb fun(): number, string
function PosTopOfFileTakesCb(cb) end
