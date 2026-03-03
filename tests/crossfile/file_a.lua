-- Cross-file test: file A defines globals on the addon namespace
local addonName, ns = ...
ns.version = 1
ns.title = "MyAddon"
ns.DB = {}
function ns:Init()
    return true
end
function ns.DB:Start()
    return 0
end
