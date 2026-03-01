-- Test: syntax errors / diagnostics

-- ExpectingThen: if without then
if true
end

-- ExpectingDo: while without do
while true
end

-- NotClosedBlock: function without end
local function unclosed()
  local x = 1

-- ExpectingExpression: assignment with no rhs
local y =

-- UnexpectedKeyword: keyword where expression expected
local z = end

-- ExpectingClosingBracket: unclosed paren
local a = (1 + 2

-- ExpectingName: dot access without name
local t = {}
t.

-- ExpectingComma: bad table constructor
local tbl = { 1 2 3 }
