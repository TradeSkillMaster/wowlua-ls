-- List element types inferred from inserts: a list built by inserting into an
-- empty table constructor takes its element type from the inserted values. A
-- call to a `@stores-into N` function passing the table as its Nth argument, a
-- `T[]` parameter, stores each argument the signature types as a bare `T`
-- (`table.insert(list, value)`), the same as `list[#list + 1] = value`.
---@diagnostic disable: unused-local, unused-function, missing-parameter

---@class ListItem
---@field name string
---@field count number

---@return ListItem?
local function findItem() return nil end

-- ── tinsert into {} ─────────────────────────────────────────────────────────

---@param item ListItem
local function build(item)
    local list = {}
    tinsert(list, item)
    local popped = tremove(list)
    --    ^ hover: (local) popped: ListItem
    --          ^ hint: : ListItem
    for _, v in ipairs(list) do
        local name = v.name
        --    ^ hover: (local) name: string
    end
    local count = list[1].count
    --    ^ hover: (local) count: number
    local typo = list[1].nam
    --                   ^ diag: undefined-field
    return list
end

local built = build()
--    ^ hover: (local) built: ListItem[]

-- table.insert with a position, through a local alias.
local insert = table.insert

---@param item ListItem
local function buildAt(item)
    local list = {}
    insert(list, 1, item)
    return list
end

local builtAt = buildAt()
--    ^ hover: (local) builtAt: ListItem[]

-- A module-level queue filled in one function and drained in another.
local queue = {}

---@param item ListItem
local function enqueue(item)
    tinsert(queue, item)
end

local function dequeue()
    return tremove(queue, 1)
end

local dequeued = dequeue()
--    ^ hover: (local) dequeued: ListItem

-- ── Element type is the union of every stored value ─────────────────────────

---@param item ListItem
local function mixed(item)
    local list = {}
    tinsert(list, item)
    tinsert(list, "label")
    return list
end

local mixedList = mixed()
--    ^ hover: (local) mixedList: (ListItem | string)[]

-- Inserts and bracket writes into the same list both count, so neither is
-- checked against an element type inferred from the other alone.
local codes = {}
codes[1] = "a"
tinsert(codes, 5)
local codesRef = codes
--    ^ hover: (local) codesRef: (string | number)[]

-- An appended nil-able value stores only its non-nil part, for an insert and a
-- `t[#t + 1]` write alike; a write to any other key can leave a nil in its slot.
local function collect()
    local list = {}
    tinsert(list, findItem())
    local found = list[1]
    --    ^ hover: (local) found: ListItem
    return list
end

local collected = collect()
--    ^ hover: (local) collected: ListItem[]

local written = {}
written[#written + 1] = findItem()
local writtenRef = written
--    ^ hover: (local) writtenRef: ListItem[]

local function byIndex(n)
    local results = {}
    for i = 1, n do results[i] = findItem() end
    local result = results[1]
    --    ^ hover: (local) result: ListItem?
end

-- Table-constructor values and nested lists.
local function rows()
    local result = {}
    local row = {}
    tinsert(row, 1)
    tinsert(result, row)
    local records = {}
    tinsert(records, { id = 1, label = "x" })
    return result, records
end

local nested, records = rows()
--    ^ hover: (local) nested: number[][]
--            ^ hover: (local) records: {id: number, label: string}[]

-- A comparator's parameters take the inferred element type.
---@param item ListItem
local function sorted(item)
    local list = {}
    tinsert(list, item)
    table.sort(list, function(a, b) return a.count < b.count end)
    --                        ^ hover: (param) a: ListItem
end

-- Any `@generic` function can be marked `@stores-into`, not just `table.insert`.
---@generic T
---@param list T[]
---@param value T
---@stores-into 1
local function push(list, value) end

---@param item ListItem
local function viaPush(item)
    local list = {}
    push(list, item)
    return list
end

local pushed = viaPush()
--    ^ hover: (local) pushed: ListItem[]

-- ── Negative controls ───────────────────────────────────────────────────────

-- An annotated list keeps its declared element type; inserts are checked against it.
---@type ListItem[]
local declared = {}
tinsert(declared, "not an item")
--                ^ diag: type-mismatch

-- A constructor's own elements fix the element type; inserts are checked against it.
local letters = { "a" }
tinsert(letters, 5)
--               ^ diag: type-mismatch

-- A generic call with no `T`-typed argument stores nothing.
local untouched = {}
table.sort(untouched)
local untouchedRef = untouched
--    ^ hover: (local) untouchedRef: table

-- A value of unknown type leaves the element type unknown.
local function addUnknown(value)
    local list = {}
    tinsert(list, value)
    return list
end

local unknownList = addUnknown()
--    ^ hover: (local) unknownList: table

-- ...also for the reads that resolve before the list's type is settled: one
-- untyped value among typed ones types the elements as nothing, not as the
-- typed values alone.
local tracked = {}
tinsert(tracked, findItem())
local function track(value) tinsert(tracked, value) end
local function eachTracked()
    for _, v in ipairs(tracked) do v:Refresh() end
end

local written2 = {}
written2[#written2 + 1] = findItem()
local function track2(value) written2[#written2 + 1] = value end
local function eachWritten()
    for _, v in ipairs(written2) do v:Refresh() end
end

-- An unmarked `(T[], T)` generic is a lookup, not a store: `T` binds from the
-- list, the value is checked against it, and the list keeps its element type.
---@generic T
---@param list T[]
---@param value T
---@return boolean
local function contains(list, value) return false end

local names = {}
names[#names + 1] = "a"
local hasFive = contains(names, 5)
--                              ^ diag: type-mismatch
local firstName = names[1]
--    ^ hover: (local) firstName: string

-- Malformed: the parameter index must be a number >= 1.
---@stores-into list
--  ^ diag: malformed-annotation
local function namedStoresInto() end

---@stores-into 0
--  ^ diag: malformed-annotation
local function zeroStoresInto() end
