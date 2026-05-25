---@diagnostic disable: undefined-global
-- Cross-file negative test: non-defclass methods with class-name string arguments
-- must NOT cause the variable to be typed as that class. Chained calls resolve
-- through the normal resolve phase (FieldAccess + backtick generic binding),
-- so only methods with actual @defclass or @generic T + @return T produce
-- class-typed results.

local FcUtils = select(2, ...).FalseChainUtils

-- NEGATIVE: Lookup returns `table` (concrete, no generics).
local a = FcUtils:Lookup("FalseChainTarget")
--    ^ hover: (local) a: table  diag: unused-local

-- NEGATIVE: Tag returns self, Lookup returns table.
local b = FcUtils:Tag("x"):Lookup("FalseChainTarget")
--    ^ hover: (local) b: table  diag: unused-local

-- POSITIVE: Create IS @defclass.
local c = FcUtils:Create("FalseChainTarget")
--    ^ hover: (local) c: FalseChainTarget {  diag: unused-local

-- POSITIVE: Tag returns self, Create is @defclass.
local d = FcUtils:Tag("x"):Create("FalseChainTarget")
--    ^ hover: (local) d: FalseChainTarget {  diag: unused-local

-- NEGATIVE: Create returns FalseChainTarget which has no Lookup method.
local e = FcUtils:Create("FalseChainTarget"):Lookup("FalseChainTarget")
--    ^ hover: (local) e: ?  diag: unused-local

-- NEGATIVE: FalseChainOther:Create returns number (not @defclass).
local FcOther = select(2, ...).FalseChainOther
local f = FcOther:Create("FalseChainTarget"):Lookup("FalseChainTarget")
--    ^ hover: (local) f: ?  diag: unused-local

-- NEGATIVE: Setup returns table, Create on FalseChainOther returns number.
local g = FcOther:Setup("FalseChainTarget"):Create("FalseChainTarget")
--    ^ hover: (local) g: ?  diag: unused-local

-- NEGATIVE: Same through addon namespace.
local h = select(2, ...).FalseChainOther:Setup("FalseChainTarget"):Create("FalseChainTarget")
--    ^ hover: (local) h: ?  diag: unused-local

-- NEGATIVE: FalseChainGlobal:Setup returns table, not a class.
local i = FalseChainGlobal:Setup("FalseChainTarget"):Create("FalseChainTarget")
--    ^ hover: (local) i: ?  diag: unused-local
