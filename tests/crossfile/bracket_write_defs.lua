-- Cross-file test: bracket-access writes should not override field type
local _, ns = ...

ns.currIds = {}
ns.currIndexes = {}

ns.currIds["adventurerCrest"] = 3383
ns.currIndexes[ns.currIds.adventurerCrest] = true
