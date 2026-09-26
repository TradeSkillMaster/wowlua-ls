-- `Owner` and `Helper` are declared together, but `owner_lib.lua` also assigns an
-- `Owner` field, so a harvest of `Owner` analyzes a file `Helper`'s doesn't — and
-- `Helper:Init` reads that file.
local addonName, ns = ...

---@class Owner
ns.Owner = {}

---@class Helper
ns.Helper = {}

function ns.Helper:Init()
    self.amount = ns.Lib.Get()
end
