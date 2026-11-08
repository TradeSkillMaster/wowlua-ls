-- `MakeThing` here is a file-local function, not the global of the same name:
-- other files read its return.
local function MakeThing() return 5 end
NS.SHADOWED = { A = MakeThing() }
