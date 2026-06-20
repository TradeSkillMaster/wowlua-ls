-- Cross-file constrained parameterized alias: the alias and its constraint
-- classes are defined here; alias_constraint_user.lua consumes the alias and
-- the `@alias Foo<T: Constraint>` bound must be enforced across files.

---@class ACAnimal
---@field name string

---@class ACDog : ACAnimal
---@field breed string

---@class ACRock
---@field hardness number

---@alias ACWrap<T: ACAnimal> { value: T }
