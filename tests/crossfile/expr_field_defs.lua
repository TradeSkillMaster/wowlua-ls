-- Cross-file test: addon namespace fields assigned from expressions
local _, ns = ...

-- String concat fields
ns.PATTERNS = {}
ns.PATTERNS.SEP = " ,;:"
ns.PATTERNS.PAT_SEP = "[" .. ns.PATTERNS.SEP .. "]"
ns.PATTERNS.PAT_INV = "[^" .. ns.PATTERNS.SEP .. "]+"
ns.PATTERNS.JOINED = ns.PATTERNS.SEP .. ns.PATTERNS.PAT_SEP

-- Arithmetic fields
ns.CONSTANTS = {}
ns.CONSTANTS.BASE = 10
ns.CONSTANTS.DOUBLED = ns.CONSTANTS.BASE * 2
ns.CONSTANTS.OFFSET = ns.CONSTANTS.BASE + 5

-- Unary expression fields
ns.CONSTANTS.NEG = -ns.CONSTANTS.BASE
ns.CONSTANTS.LEN = #ns.PATTERNS.SEP

-- And-chain fields: RHS type should be used
ns.GUARDED = {}
ns.GUARDED.STR = ns.PATTERNS and "fallback"
ns.GUARDED.NUM = ns.CONSTANTS and 99
ns.GUARDED.CONCAT = ns.PATTERNS and ("prefix" .. ns.PATTERNS.SEP)
ns.GUARDED.DEEP = ns.PATTERNS and ns.CONSTANTS and "deep"

-- Global variable with concat
MY_CONCAT_RESULT = "hello" .. " " .. "world"
MY_ARITH_RESULT = 1 + 2

-- Global variable with and-chain
MY_GUARDED = ns.PATTERNS and "guarded"
