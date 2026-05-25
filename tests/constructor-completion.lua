---@diagnostic disable: undefined-global
-- Table constructor field completions: when typing inside a table constructor
-- whose expected type is a known class, offer that class's fields.

-- ── Case 1: @type annotation on local variable ──────────────────────────────

---@class CCItem
---@field name string
---@field count number
---@field active boolean

---@type CCItem
local item = {
    n
--  ^ comp: name, count, active
}

-- ── Case 2: table<K, V> value type via bracket assignment ───────────────────

---@type table<integer, CCItem>
local items = {}

items[1] = {
    n
--  ^ comp: name, count, active
}

-- ── Case 3: function parameter typed as class ───────────────────────────────

---@param data CCItem
local function processItem(data)
end

---@diagnostic disable-next-line: type-mismatch
processItem({
    n
--  ^ comp: name, count, active
})

-- ── Case 4: already-set fields are excluded ─────────────────────────────────

---@type CCItem
---@diagnostic disable-next-line: assign-type-mismatch, missing-fields
local partial = {
    name = "hello",
    a
--  ^ comp: count, active
}

-- ── Case 5: inherited fields from parent class ──────────────────────────────

---@class CCBase
---@field id number

---@class CCChild : CCBase
---@field label string

---@type CCChild
local child = {
    l
--  ^ comp: id, label
}

-- ── Case 6: callbacks included, methods excluded ────────────────────────────

---@class CCWidget
---@field width number
---@field onClick fun()
---@field render fun(self: CCWidget)

---@type CCWidget
local widget = {
    w
--  ^ comp: width, onClick
}
