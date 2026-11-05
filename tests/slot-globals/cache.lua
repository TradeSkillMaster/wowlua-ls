---@diagnostic disable: unused-local
-- An `or` whose first operand is untyped writes its second, as the fixpoint reads it.
function JournalApp.InitCache()
    JournalApp.cache = JournalApp.loaded or { "first" }
end

function JournalApp.UseCache()
    local cache = JournalApp.cache
--        ^ hover: (local) cache: string[]
    return cache
end
