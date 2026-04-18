-- Test: stylistic HINT diagnostics — empty-block, redundant-return, trailing-space

---@diagnostic disable: undefined-global, unused-local, unused-function

-- ── empty-block ─────────────────────────────────────────────────────────────

local cond = true

if cond then end
-- ^ diag: empty-block

if cond then
-- ^ diag: empty-block
end

if cond then
    print("a")
-- ^ diag: none
end

if cond then
-- ^ diag: empty-block
elseif not cond then
-- ^ diag: empty-block
else
-- ^ diag: empty-block
end

if cond then
    print("a")
-- ^ diag: none
elseif not cond then
    print("b")
-- ^ diag: none
else
    print("c")
-- ^ diag: none
end

while cond do end
-- ^ diag: empty-block

while cond do
    break
-- ^ diag: none
end

for i = 1, 10 do end
-- ^ diag: empty-block

for i = 1, 10 do
    print(i)
-- ^ diag: none
end

for _, v in ipairs({1, 2}) do end
-- ^ diag: empty-block

for _, v in ipairs({1, 2}) do
    print(v)
-- ^ diag: none
end

repeat until cond
-- ^ diag: empty-block

repeat
    print("once")
-- ^ diag: none
until cond

-- A single `break` or `return` still counts as a statement (not empty).
for i = 1, 10 do return end
-- ^ diag: none

-- empty-block suppression via disable-next-line
---@diagnostic disable-next-line: empty-block
if cond then end
-- ^ diag: none

-- ── redundant-return ────────────────────────────────────────────────────────

local function bareReturn()
    print("x")
    return
--  ^ diag: redundant-return
end

local function returnsValue()
    return 1
--  ^ diag: none
end

local function returnsNil()
    return nil
--  ^ diag: none
end

local function earlyReturn(x)
    if x then return end
    --        ^ diag: none
    print("after")
end

local function returnInBranch(x)
    if x then
        return
    --  ^ diag: none
    end
    print("past")
end

local function noReturnStatement()
    print("done")
--  ^ diag: none
end

-- redundant-return suppression via disable-next-line
local function suppressed()
    print("x")
    ---@diagnostic disable-next-line: redundant-return
    return
--  ^ diag: none
end

-- ── trailing-space ──────────────────────────────────────────────────────────

local cleanLine = "no trailing"
--    ^ diag: none

local dirtyLine = "has trailing"   
--    ^ diag: trailing-space

local anotherDirty = "also trailing"	
--    ^ diag: trailing-space

local spacedOutCode = 42
--    ^ diag: none

-- Blank lines with only whitespace should NOT fire trailing-space.
-- The following line intentionally contains only whitespace characters:
    
-- (line above has only spaces — no diagnostic is asserted against it because
-- annotation assertions resolve by walking up past blank lines, but the
-- implementation skips entirely-blank lines so no diagnostic is produced.)

-- trailing-space suppression via disable-next-line
---@diagnostic disable-next-line: trailing-space
local suppressedTrailing = "ok"  
--    ^ diag: none
