-- Namespace data built over two statements, the map typed by its entries.
local ns = select(2, ...)

ns.Constants = {
    Enabled = true,
}

ns.Constants.KindNames = {
    [Enum.PowerType.Mana] = "mana",
    [Enum.PowerType.Rage] = "rage",
}

-- A table another file adds members to.
ns.Toolkit = {}
