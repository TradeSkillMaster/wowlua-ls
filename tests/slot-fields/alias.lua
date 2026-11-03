-- Deep writes through a file-level alias of another file's global.
local App = LedgerApp

function App.Open()
    App.Window = {}
    App.Window.mode = "edit"
end
