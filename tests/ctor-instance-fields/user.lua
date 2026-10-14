---@diagnostic disable: unused-local
-- The fields constructors in defs.lua and annotated.lua set on their instances
-- are the class's fields here too.

local _, ns = ...

local h = ns.NewHolder("a")
--    ^ hover: (local) h: Holder {
local count = h.count
--    ^ hover: (local) count: number
--              ^ def: external  comp: Create, Describe, Reset, __index, built, claimed, count, created, id, label, name, owner, resets, root, size, timer, viaLocalMt
local size = h.size
--    ^ hover: (local) size: number
--             ^ def: external
local name = h.name
--    ^ hover: (local) name: string
local owner = h.owner
--    ^ hover: (local) owner: any
local root = h.root
--    ^ hover: (local) root: Holder {
local timer = h.timer
--    ^ hover: (local) timer: any
local typo = h.cuont
--             ^ diag: undefined-field

---@param holder Holder
local function Read(holder)
    local c = holder.count
    --    ^ hover: (local) c: number
    local claimed = holder.claimed
    --    ^ hover: (local) claimed: boolean
    local resets = holder.resets
    --    ^ hover: (local) resets: number
    local built = holder.built
    --    ^ hover: (local) built: boolean
    local viaLocalMt = holder.viaLocalMt
    --    ^ hover: (local) viaLocalMt: number
end

---@param holder Holder
local function Clear(holder)
    holder.count = nil
    holder.size = "big"
    --            ^ diag: field-type-mismatch
end

---@param apple Apple
---@param berry Berry
local function ReadFruit(apple, berry)
    local seeds, core, ripe = apple.seeds, apple.core, apple.ripe
    local juice, pulp, berryRipe = berry.juice, berry.pulp, berry.ripe
    local notApple = apple.juice
    --                     ^ diag: undefined-field
    local notBerry = berry.seeds
    --                     ^ diag: undefined-field
end

---@param slot Slot
local function ReadSlot(slot)
    local value = slot.value
    --    ^ hover: (local) value: string | number
end

---@param clicker Clicker
local function Click(clicker)
    clicker:OnClick("x")
    --              ^ diag: type-mismatch
end

---@param bar Bar
local function ReadBar(bar)
    local viaAssign = bar.viaAssign
    --    ^ hover: (local) viaAssign: number
end

local label = ns.NewLiteral().label
--    ^ hover: (local) label: string

local found = ns.FindHolder(1)
--    ^ hover: (local) found: Holder?
if found then
    local id = found.id
    --    ^ hover: (local) id: number?
end

local created = ns.Holder:Create().created
--    ^ hover: (local) created: boolean

local c = ns.NewCounter()
--    ^ hover: (local) c: Counter {

---@param t Typed
local function ReadTyped(t)
    local n = t.count
    --    ^ hover: (local) n: number
end

---@param p Proto
local function ReadProto(p)
    local pname = p.name
    --    ^ hover: (local) pname: string
    local pid = p.id
    --    ^ hover: (local) pid: number
    local pextra = p.extra
    --    ^ hover: (local) pextra: boolean
end

---@param o Outer
local function ReadInner(o)
    local inner = o.inner
    --    ^ hover: (local) inner: Inner & { a: number, w: true, x: number } {
end

local num = ns.num
--    ^ hover: (local) num: number
local defaultLabel = ns.defaultHolder.label
--    ^ hover: (local) defaultLabel: string
