-- Cross-file negative test: defines a non-defclass function that takes a string
-- argument. Verifies that chaining through a non-defclass method with a class-name
-- argument does NOT cause the variable to be typed as that class.
local _, ns = ...

---@class FalseChainTarget

---@class FalseChainUtils
ns.FalseChainUtils = {}

-- This method is NOT @defclass — it just happens to take a string argument.
-- A string argument matching a class name ("FalseChainTarget") must NOT cause
-- the caller variable to resolve as that class.
---@param name string
---@return table
function ns.FalseChainUtils:Lookup(name)
    return {}
end

-- Another non-defclass method returning self for chaining
---@param tag string
---@return self
function ns.FalseChainUtils:Tag(tag)
    return self
end

-- This IS a @defclass method — used to verify the positive case still works
---@generic T
---@defclass T
---@param name `T`
---@return T
function ns.FalseChainUtils:Create(name)
    return {}
end

-- A separate class whose non-defclass method shares a name ("Create") with the
-- @defclass method above. Tests the false-positive where the method name matches
-- globally even though THIS class's Create is not @defclass.
---@class FalseChainOther
ns.FalseChainOther = {}

-- NOT @defclass — just a regular method that happens to be called "Create"
---@param name string
---@return number
function ns.FalseChainOther:Create(name)
    return 0
end

-- Non-defclass inner method, with an outer method whose name ("Create") collides
-- with a @defclass method on FalseChainUtils.
---@param label string
---@return table
function ns.FalseChainOther:Setup(label)
    return {}
end

-- A global-scoped class to test direct ClassName:Method chains
---@class FalseChainGlobal
FalseChainGlobal = {}

-- Non-defclass method whose name does NOT collide
---@param name string
---@return table
function FalseChainGlobal:Setup(name)
    return {}
end

-- Non-defclass method whose name ("Create") DOES collide with a @defclass method
---@param name string
---@return string
function FalseChainGlobal:Create(name)
    return ""
end
