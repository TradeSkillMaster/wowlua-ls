-- Test: stylistic HINT diagnostics — empty-block, redundant-return, trailing-space

---@diagnostic disable: undefined-global, unused-local, unused-function

-- ── empty-block ─────────────────────────────────────────────────────────────

local cond = true

if cond then end
-- ^ diag: empty-block

if cond then
    print("a")
end

if cond then print("a") elseif not cond then end
-- ^ diag: empty-block

if cond then print("a") else end
-- ^ diag: empty-block

if cond then
    print("a")
elseif not cond then
    print("b")
else
    print("c")
end

while cond do end
-- ^ diag: empty-block

while cond do
    break
end

for i = 1, 10 do end
-- ^ diag: empty-block

for i = 1, 10 do
    print(i)
end

for _, v in ipairs({1, 2}) do end
-- ^ diag: empty-block

for _, v in ipairs({1, 2}) do
    print(v)
end

repeat until cond
-- ^ diag: empty-block

repeat
    print("once")
until cond

-- A single `break` or `return` still counts as a statement (not empty).
for i = 1, 10 do return end

-- empty-block suppression via disable-next-line
---@diagnostic disable-next-line: empty-block
if cond then end

-- A short comment inside an otherwise-empty block marks an intentional
-- fall-through and suppresses empty-block (matches sumneko/LuaLS).
if cond then
    -- pass
end

if cond then
    -- Moving to the same index
elseif not cond then
    -- Do nothing
else
    -- TODO: handle this case
end

while cond do
    -- just ignore this
end

for i = 1, 10 do
    -- continue looping
end

for _, v in ipairs({1, 2}) do
    -- skip
end

repeat
    -- no-op
until cond

-- A long-bracket comment also suppresses empty-block.
if cond then
    --[[ intentional fall-through ]]
end

-- ── redundant-return ────────────────────────────────────────────────────────

local function bareReturn()
    print("x")
    return
--  ^ diag: redundant-return
end

local function returnsValue()
    return 1
end

local function returnsNil()
    return nil
end

local function earlyReturn(x)
    if x then return end
    print("after")
end

local function returnInBranch(x)
    if x then
        return
    end
    print("past")
end

local function noReturnStatement()
    print("done")
end

-- redundant-return suppression via disable-next-line
local function suppressed()
    print("x")
    ---@diagnostic disable-next-line: redundant-return
    return
end

-- ── trailing-space ──────────────────────────────────────────────────────────

local cleanLine = "no trailing"

local dirtyLine = "has trailing"   
--    ^ diag: trailing-space

local anotherDirty = "also trailing"	
--    ^ diag: trailing-space

local spacedOutCode = 42

-- Blank lines with only whitespace should NOT fire trailing-space.
-- The following line intentionally contains only whitespace characters:
    
-- (line above has only spaces — no diagnostic is asserted against it because
-- annotation assertions resolve by walking up past blank lines, but the
-- implementation skips entirely-blank lines so no diagnostic is produced.)

-- trailing-space suppression via disable-next-line
---@diagnostic disable-next-line: trailing-space
local suppressedTrailing = "ok"  
