---@diagnostic disable: unused-local, unused-function, undefined-global

---@return string?
local function maybeName() return nil end

---@param n nil
local function takesNil(n) end

---@param s string
local function takesString(s) end

-- An early-exit guard narrows the code after the chain. The exiting branch
-- itself still sees the pre-guard value: inside `if not name then`, `name` is
-- nil, not the truthy type it has past the guard. Neither the guard's own
-- condition nor a read before it is narrowed.
local function test_truthy_exit_branch_read()
    local name = maybeName()
    local before = name
    --             ^ hover: (local) name: string?
    if not name then
    --     ^ hover: (local) name: string?
        takesNil(name)
        --       ^ hover: (local) name: nil
        return
    end
    takesString(name)
    --          ^ hover: (local) name: string
end

-- Same for blocks and closures nested inside the exiting branch.
local function test_truthy_exit_nested_read(cond)
    local name = maybeName()
    if not name then
        if cond then
            takesNil(name)
            --       ^ hover: (local) name: nil
        end
        local function report()
            takesNil(name)
        end
        report()
        return
    end
    takesString(name)
end

-- `if a or not b then return a end`: past the guard `a` is falsy, but inside
-- the branch it can be either (`a` truthy, or `b` missing), so `return a`
-- returns the full `boolean`.
---@return boolean
---@return string?
local function validate()
    return true, nil
end

local function checkValid()
--             ^ hover: (local) function checkValid()\n-> boolean?, string?\ncases (inferred):\n(boolean, nil)\n(false?, string)
    local isValid, errType = validate()
    if isValid or not errType then
        return isValid
    elseif errType == "range" then
        return false, "out of range"
    else
        return nil, "invalid"
    end
end

-- The caller narrows the result of that guard-typed return truthy.
local function useValid()
    local isValid, err = checkValid()
    if not isValid then
        return err
    end
    local ok = isValid
    --    ^ hover: (local) ok: true
    return ok
end

-- A reassignment inside the exiting branch is dead past the guard: the code
-- after it sees the narrowed original value, not the branch's assignment.
local function test_exit_branch_reassign()
    local name = maybeName()
    if not name then
        name = 1
        return
    end
    local n = name
    --    ^ hover: (local) n: string
end

-- The same local re-declared `@class` inside the exiting branch: past the guard
-- the writes go through the first declaration, and they still resolve through
-- the class name, which names the last declaration.
---@return table?
local function fetchNode() return nil end

---@return ExitScopeNode
local function getNode()
    --- @class ExitScopeNode
    local node = fetchNode()
    if not node then
        --- @class ExitScopeNode
        node = fetchNode()
        if not node then error("missing") end
        return node
    end
    node.rank = 1
    return node
end

local function readRank()
    local node = getNode()
    return node.rank
    --          ^ hover: (field) rank: number
end

---@class ExitScopeHolder
---@field width number
---@field label string?

---@param fmt string
---@param ... string|number|boolean
local function logErr(fmt, ...) end

-- `if not t.f then t.f = v end` narrows `t.f` non-nil after the block, not
-- inside it: before the write, the branch still reads the declared type.
---@param h ExitScopeHolder
local function test_ensure_initialized(h)
    if not h.label then
        local before = h.label
        --    ^ hover: (local) before: string?
        h.label = "default"
    end
    local after = h.label
    --    ^ hover: (local) after: string
end

-- A defensive `type()` check on a field declared `number`: the branch is
-- unreachable by the declared type, so the value logged there is not `nil`.
---@param h ExitScopeHolder
local function test_defensive_type_check(h)
    if type(h.width) ~= "number" then
        logErr("bad width %s", h.width)
        return false
    end
    return true
end

---@return number?
local function maybeCount() return nil end

-- `local y = t.f and expr; if y then` proves `t.f` truthy inside, like the
-- plain-local form of the implication.
---@param h ExitScopeHolder
local function test_and_field_derivation(h)
    local len = h.label and #h.label
    if len then
        local label = h.label
        --    ^ hover: (local) label: string
    end
end

-- A reassignment inside a falsy region (`if not name then name = … end`) ends
-- that region's narrowing.
local function test_falsy_region_reassign()
    local name = maybeName()
    if not name then
        name = maybeName()
        local retry = name
        --    ^ hover: (local) retry: string?
    end
end

-- After `if count then return count end`, count is falsy — until a later branch
-- gives it a value.
local function test_falsy_fact_then_branch_assign(cond)
    local count = maybeCount()
    if count then
        return count
    end
    local falsy = count
    --    ^ hover: (local) falsy: nil
    if cond then
        count = 5
    end
    local merged = count
    --    ^ hover: (local) merged: number?
    return merged
end

---@class ExitScopeRow
local ExitScopeRow = {}
---@return false
function ExitScopeRow:IsSubRow() return false end

---@class ExitScopeSubRow
local ExitScopeSubRow = {}
---@return true
function ExitScopeSubRow:IsSubRow() return true end

---@return ExitScopeRow
local function plainRow() return ExitScopeRow end

-- A literal-boolean method guard (`if not row:IsSubRow() then`) discriminates
-- the receiver's value before the exiting branch, not the branch's reassignment.
---@param row ExitScopeRow|ExitScopeSubRow
local function test_bool_discriminator_exit(row)
    if not row:IsSubRow() then
        row = plainRow()
        return
    end
    local after = row
    --    ^ hover: (local) after: ExitScopeSubRow
end

---@class ExitScopeTitled
---@field title string

---@class ExitScopeUntitled
---@field id number

-- A second exit guard on the same variable applies its facts after itself, not
-- from the first guard on.
---@param info ExitScopeTitled|ExitScopeUntitled|nil
local function test_second_guard_extent(info)
    if not info then return end
    local before = info
--                 ^ hover: (param) info: ExitScopeTitled | ExitScopeUntitled
    if not info.title then return end
    local after = info
    --    ^ hover: (local) after: ExitScopeTitled
end
