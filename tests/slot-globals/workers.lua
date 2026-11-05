---@diagnostic disable: create-global
-- A table built in a function, its fields written through it: a field one file
-- writes is that file's in the others too, though another file builds the table.
---@class JournalWorker
---@field busy boolean

---@return JournalWorker
function CreateJournalWorker()
    return { busy = false }
end

JournalApp = {}

function JournalApp.InitScan()
    JournalApp.Workers = {}
    JournalApp.Workers.scan = CreateJournalWorker()
end
