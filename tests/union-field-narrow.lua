---@diagnostic disable: undefined-global, unused-function, unused-local
-- Tests for union member narrowing based on field presence/truthiness guards

---@class ColInfo.WithTitle
---@field title string
---@field justifyH string

---@class ColInfo.WithIcon
---@field titleIcon string
---@field justifyH string

---@alias ColInfo ColInfo.WithTitle | ColInfo.WithIcon

-- ── Then/else branch narrowing via bare truthiness ───────────��──────────────

---@param info ColInfo
local function test_if_else(info)
    if info.title then
        local t = info
        --    ^ hover: (local) t: ColInfo.WithTitle
        local x = info.title
        --             ^ hover: (field) title: string
    else
        local t = info
        --    ^ hover: (local) t: ColInfo.WithIcon
        local x = info.titleIcon
        --             ^ hover: (field) titleIcon: string
    end
end

-- ── Early exit: if not info.title then return end ───────────��───────────────

---@param info ColInfo
local function test_early_exit(info)
    if not info.title then return end
    local t = info
    --    ^ hover: (local) t: ColInfo.WithTitle
end

-- ── Early exit: if info.title then return end ───────────────────────────────

---@param info ColInfo
local function test_early_exit_truthy(info)
    if info.title then return end
    local t = info
    --    ^ hover: (local) t: ColInfo.WithIcon
end

-- ── Assert pattern ──────────────────────────────────────��───────────────────

---@param info ColInfo
local function test_assert(info)
    assert(info.title)
    local t = info
    --    ^ hover: (local) t: ColInfo.WithTitle
end

-- ── Nil comparison: info.title ~= nil ──────────��────────────────────────────

---@param info ColInfo
local function test_nil_neq(info)
    if info.title ~= nil then
        local t = info
        --    ^ hover: (local) t: ColInfo.WithTitle
    else
        local t = info
        --    ^ hover: (local) t: ColInfo.WithIcon
    end
end

-- ── Nil comparison: info.title == nil ──────────────��────────────────────────

---@param info ColInfo
local function test_nil_eq(info)
    if info.title == nil then
        local t = info
        --    ^ hover: (local) t: ColInfo.WithIcon
    else
        local t = info
        --    ^ hover: (local) t: ColInfo.WithTitle
    end
end

-- ── Early exit with nil comparison ──────────────────────────────────────────

---@param info ColInfo
local function test_nil_early_exit(info)
    if info.title == nil then return end
    local t = info
    --    ^ hover: (local) t: ColInfo.WithTitle
end

-- ── No narrowing when all members have the field as required ────────────────

---@class Both.A
---@field shared string
---@field onlyA number

---@class Both.B
---@field shared string
---@field onlyB number

---@alias BothHaveShared Both.A | Both.B

---@param x BothHaveShared
local function test_no_narrow_shared(x)
    if x.shared then
        local t = x
        --    ^ hover: (local) t: Both.A | Both.B
    end
end

-- ── Three-way union narrows to subset ────────────���──────────────────────────

---@class Shape.Circle
---@field radius number

---@class Shape.Rect
---@field width number
---@field height number

---@class Shape.Line
---@field length number
---@field width number

---@alias Shape Shape.Circle | Shape.Rect | Shape.Line

---@param s Shape
local function test_three_way(s)
    if s.width then
        local t = s
        --    ^ hover: (local) t: Shape.Rect | Shape.Line
    else
        local t = s
        --    ^ hover: (local) t: Shape.Circle
    end
end

-- ── Optional field is retained in then-branch (can still be truthy) ──────────

---@class Opt.A
---@field tag string

---@class Opt.B
---@field tag string?
---@field extra number

---@alias OptUnion Opt.A | Opt.B

---@param x OptUnion
local function test_optional_field(x)
    if x.tag then
        local t = x
        --    ^ hover: (local) t: Opt.A | Opt.B
    else
        local t = x
        --    ^ hover: (local) t: Opt.B
    end
end

-- ── Field chain string literal early-exit narrowing ─────────────────────────

---@class StatusObj
---@field status "active"|"inactive"|"archived"
---@field value number

-- `if obj.status == "archived" then return end` strips "archived" from field
---@param obj StatusObj
local function test_field_literal_eq_early_exit(obj)
    if obj.status == "archived" then
        return
    end
    local _s = obj.status
    --    ^ hover: (local) _s: "active" | "inactive"
end

-- `if obj.status ~= "active" then return end` filters field to "active"
---@param obj StatusObj
local function test_field_literal_neq_early_exit(obj)
    if obj.status ~= "active" then
        return
    end
    local _s = obj.status
    --    ^ hover: (local) _s: "active"
end

---@param s "active"
local function takesActive(s) end

-- ...and only past the guard: a read before it still sees the whole union.
---@param obj StatusObj
local function test_field_literal_neq_not_retroactive(obj)
    takesActive(obj.status)
    --          ^ diag: type-mismatch
    if obj.status ~= "active" then return end
    takesActive(obj.status)
end

-- A literal comparison on a bracket access (`res[#res] == "nil"`) narrows the
-- element, not the table: `res` stays an array, it does not gain the literal.
local function test_bracket_literal_guard_keeps_array_type()
    local res = {}
    res[#res + 1] = "x"
    while res[#res] == "nil" do
        res[#res] = nil
    end
    local _r = res
    --    ^ hover: (local) _r: string[]
end

-- ── Literal early-exit strips only past the guard ──────────────────────────

---@param n number
local function takesNum(n) end

---@return "SKIP"|number
local function readSlot() return 1 end

-- The stripped member is still live on reads *before* the guard.
local function test_literal_early_exit_not_retroactive()
    local slot = readSlot()
    takesNum(slot)
    --       ^ diag: type-mismatch
    if slot == "SKIP" then return end
    takesNum(slot)
end

---@param s "SKIP"
local function takesSkip(s) end

-- Same for the `~=` direction, which filters to the literal past the guard.
local function test_literal_neq_early_exit_not_retroactive()
    local slot = readSlot()
    takesSkip(slot)
    --        ^ diag: type-mismatch
    if slot ~= "SKIP" then return end
    takesSkip(slot)
end

-- A read of the guarded symbol inside the exiting branch must not carry its
-- branch-local narrowing out past the guard.
local function test_literal_early_exit_with_branch_read()
    local slot = readSlot()
    if slot == "SKIP" then
        return slot
    end
    takesNum(slot)
end
