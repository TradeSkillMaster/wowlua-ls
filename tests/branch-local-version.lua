-- Regression: branch merge must not create versions for branch-local variables
-- in the parent scope. When a variable is declared with `local` inside a branch
-- (e.g. from multi-return destructuring inside an if-body), the branch merge
-- should skip it. Otherwise overload narrowing chains through the spurious merge
-- version and overwrites the correct reassignment version.

-- Pattern 1: Local from multi-return, then reassigned with or-coalesce
---@return string
---@return string?
---@return number?
local function getData()
    return "ok", nil, nil
end

local reason, seller, auctionId = getData()
seller = seller or ""
auctionId = auctionId or 0
local _x = seller .. "test"
--         ^ hover: (local) seller: string  diag: none

-- Pattern 2: Local nil, assigned in branches, conditionally reassigned, then narrowed
-- Mirrors `stackLine = numSubs > 0 and parsedStackLine or nil` from real code.
local stackLine = nil
if true then
    stackLine = "hello"
else
    stackLine = "world"
end
local numSubs = 1
stackLine = numSubs > 0 and stackLine or nil
--^ hover: (local) stackLine: string?
if stackLine then
    local _y = stackLine .. "!"
    --         ^ hover: (local) stackLine: string  diag: none
end

-- Pattern 3: Inside a while loop
local level = 1
while true do
    local line = nil
    if level > 0 then
        line = "test"
    else
        line = "other"
    end
    line = level > 0 and line or nil
    --^ hover: (local) line: string?
    if line then
        local _z = line .. "!"
        --         ^ hover: (local) line: string  diag: none
    end
    level = level + 1
end
