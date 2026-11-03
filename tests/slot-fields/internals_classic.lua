-- Two files build the same module table, one per game flavor: other files read
-- one table with the fields of both.
LedgerApp.Internals = {}
LedgerApp.Internals.Queue = {}
LedgerApp.Internals.Page = 1

-- Each flavor's file builds the layout in a function: other files read one table,
-- with a field one of them lacks optional.
function LedgerApp.LoadLayout()
    LedgerApp.Layout = { rows = 2 }
end
