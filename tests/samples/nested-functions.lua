-- Test: nested function return tracking
-- The old code used functions.len()-1 which would
-- misattribute inner return to outer function

local function outer()
	local function inner()
		return "hello"
	end
	return inner()
end
