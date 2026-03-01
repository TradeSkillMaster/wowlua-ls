-- Test: all statement types handled in collect_identifiers

-- While loop
local counter = 0
while counter < 10 do
	local inside_while = "hello"
	counter = counter + 1
end

-- Repeat loop
repeat
	local inside_repeat = 42
until true

-- If/elseif/else
local cond = true
if cond then
	local inside_if = "if_branch"
elseif not cond then
	local inside_elseif = "elseif_branch"
else
	local inside_else = "else_branch"
end

-- For count loop
for i = 1, 10 do
	local inside_for = i + 1
end

-- For-in loop
for k, v in pairs({}) do
	local inside_forin = "iterating"
end

-- Global assignment
globalVar = "I am global"
globalNum = 123

-- Global function assignment
globalFunc = function(a, b)
	return a + b
end
