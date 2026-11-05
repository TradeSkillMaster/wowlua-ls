---@diagnostic disable: create-global
-- Saved variables an event handler initializes, written in several functions and
-- files, and read in other functions of this one.
local VERSION = 3

function JournalReset()
    JOURNAL_CONFIG = { Profiles = { Default = {} }, Version = 1 }
end

function JournalInit()
    if JOURNAL_CONFIG == nil then
        JournalReset()
        return
    end
    if JOURNAL_CONFIG.Profiles[JOURNAL_PROFILE] == nil then
        JOURNAL_PROFILE = "Default"
--      ^ hover: (global) JOURNAL_PROFILE: string = "Default"
    end
    -- A map the scan sees only the keys of.
    JOURNAL_CACHE = { ["__version"] = VERSION }
end

-- An unannotated parameter stored there says nothing of the global.
function JournalUseProfile(name)
    JOURNAL_PROFILE = name
end

function JournalVersion()
    local config = JOURNAL_CONFIG
--        ^ hover: (local) config: {Profiles: {Default: table}, Version: number}?
    return config
end
