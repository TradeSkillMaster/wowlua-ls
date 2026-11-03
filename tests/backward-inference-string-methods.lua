---@diagnostic disable: unused-local, unused-function
-- Backward param-type inference with the string library: a string method called
-- on the param (through the string metatable) leaves it a string, not a table; a
-- field of the same name read off it, either.

local function trimmed(name)
--                     ^ hover: (param) name: string?
    if not name then
        return
    end
    name = name:gsub("^%s+", "")
    return name
end

local function reversedStyle(style)
--                           ^ hover: (param) style: string | table?
    if not style then
        return
    end
    return style.reverse
end
