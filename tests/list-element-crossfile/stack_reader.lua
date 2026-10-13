-- A class with a constructor (here `__private:__init`) gets its fields from it:
-- one only another method writes isn't declared for other files.
---@param proc CrossPrivProc
local function readCache(proc)
    local cache = proc._cache
    --                 ^ diag: undefined-field
    return cache
end
