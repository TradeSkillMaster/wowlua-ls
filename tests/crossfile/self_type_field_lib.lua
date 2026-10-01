---@diagnostic disable: create-global
-- `self` in a @field type names the declaring class when read from another file.

---@class SelfTypeFieldData
---@field onUpdate fun(data: self): nil
---@field clone fun(): self
SelfTypeFieldData = {}
