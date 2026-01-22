local x = 5
local y = x + 2

local function AddTwo(val)
	return val + 2
end

local function GetMagicNumbers()
	return 11, 22
end

do
	local x2 = 5
	local x2y = x2 + y + 1
	local a, b = "a", "b"
	local a, b = 22, 33
	local res = AddTwo(a)
	local magic1, magic2 = GetMagicNumbers()
end
