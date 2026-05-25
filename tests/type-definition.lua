-- Tests for textDocument/typeDefinition (Go to Type Definition)

---@class Widget
---@field name string

-- Variable typed as a @class: typedef navigates to the class declaration
local w = {} ---@type Widget
--    ^ hover: (local) w: Widget  typedef: local 3:1

---@class Container
---@field count number

-- Parameter typed as @class
---@param c Container
local function useContainer(c)
    local _ = c
    --        ^ hover: (param) c: Container  typedef: local 10:1
end
local _ = useContainer

-- Class declared later in the file
---@class Node

local myNode = {} ---@type Node
--    ^ hover: (local) myNode: Node  typedef: local 22:1
_ = myNode

-- Primitive types: typedef returns None
local n = 42
--    ^ typedef: None

local s = "hello"
--    ^ typedef: None

-- Union type with a class member: navigate to first class
---@class Wrapper

local mixed = nil ---@type Wrapper | nil
--    ^ typedef: local 36:1
_ = mixed

-- OpaqueAlias: navigate to the alias declaration
---@alias (opaque) ItemId number

local itemId = 1 ---@type ItemId
--    ^ typedef: local 43:1
_ = itemId

-- Field access: typedef navigates to the field's type class
---@class Inner
---@field value number

---@class Outer
---@field child Inner

local outer = {} ---@type Outer
local inner = outer.child
--                  ^^^^^ typedef: local 50:1
_ = inner
