---@diagnostic disable: create-global
-- Another file resets the saved variable; its write shows the value it writes.
function JournalWipe()
    JOURNAL_CONFIG = nil
--  ^ hover: (global) JOURNAL_CONFIG: nil
end
