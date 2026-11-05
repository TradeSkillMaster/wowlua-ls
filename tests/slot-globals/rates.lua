---@diagnostic disable: unused-local
-- A table this file builds at a path other files fill: it reads what they write.
JournalApp.Rates = {}

function JournalApp.Rates.Get(id)
    local rates = JournalApp.Rates.TABLE
--        ^ hover: (local) rates: number[]
    return rates
end
