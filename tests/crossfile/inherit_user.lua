-- Cross-file inheritance test: child class inherits from cross-file parent

-- Define child class that inherits from cross-file InhRect (which inherits InhShape)
---@class InhCircle : InhShape
--                     ^ hover: (class) InhShape  def: external
---@field radius number

---@type InhCircle
local circ = {}

-- Own field
local r = circ.radius
--    ^ hover: (local) r: number  def: local

-- Inherited from InhShape (grandparent-level for InhRect, direct parent for InhCircle)
local col = circ.color
--    ^ hover: (local) col: string  def: local
local vis = circ.visible
--    ^ hover: (local) vis: boolean  def: local

-- Inherited method from InhShape (displayed with child class prefix)
circ:GetColor()
--   ^ hover: (method) function InhCircle:GetColor()  def: external

-- Inline annotation with cross-file parent class
local INH_METHODS = {} ---@class InhSquare : InhShape
--                                            ^ hover: (class) InhShape  def: external

-- Use cross-file child class InhRect with multi-level inheritance
---@type InhRect
local rect = {}
local w = rect.width
--    ^ hover: (local) w: number  def: local
local rc = rect.color
--     ^ hover: (local) rc: string  def: local
rect:Area()
--   ^ hover: (method) function InhRect:Area()  def: external
rect:GetColor()
--   ^ hover: (method) function InhRect:GetColor()  def: external
