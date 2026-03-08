-- Test: circular @class inheritance detection

-- Direct cycle: A -> B -> A
---@class CycleA : CycleB
-- ^ diag: circle-doc-class
---@field x number

---@class CycleB : CycleA
-- ^ diag: circle-doc-class
---@field y string

-- Self-referential: C -> C
---@class CycleSelf : CycleSelf
-- ^ diag: circle-doc-class
---@field z boolean

-- Indirect cycle: D -> E -> F -> D
---@class CycleD : CycleE
-- ^ diag: circle-doc-class
---@field d number

---@class CycleE : CycleF
-- ^ diag: circle-doc-class
---@field e number

---@class CycleF : CycleD
-- ^ diag: circle-doc-class
---@field f number

-- No cycle: normal inheritance
---@class Base
---@field base_val number

---@class Child : Base
-- ^ diag: none
---@field child_val string
