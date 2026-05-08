---@class ExprState
---@field progress number
---@field active boolean
---@field name string
---@field count number

---@param expr expression<ExprState, boolean>
function checkBool(expr) end

---@param expr expression<ExprState>
function checkAny(expr) end

---@param expr expression<ExprState, number>
function checkNum(expr) end

-- Hover on identifiers inside expression strings
checkBool([[progress == 1 and not active]])
--           ^ hover: (field) progress: number
--                                  ^ hover: (field) active: boolean

-- Undefined variable diagnostic
checkBool([[unknownVar == 1]])
--           ^ diag: undefined-field

-- Expression type mismatch: name is string, expected boolean
checkBool([[name]])
-- ^ diag: type-mismatch

-- No mismatch when return type is unconstrained
checkAny([[name]])
--          ^ hover: (field) name: string  diag: none

-- Boolean expression matches boolean constraint
checkBool([[progress == 1]])
-- ^ diag: none

-- Numeric expression matches number constraint
checkNum([[progress + count]])
-- ^ diag: none

-- Numeric expression doesn't match boolean constraint
checkBool([[progress + count]])
-- ^ diag: type-mismatch

-- Completions inside expression string
checkBool([[prog]])
--            ^ comp: progress, active, name, count

-- Regular string delimiter
checkBool("progress == 1")
--           ^ hover: (field) progress: number

-- not operator infers boolean
checkBool([[not active]])
-- ^ diag: none

-- Comparison operators infer boolean
checkBool([[count > 0]])
-- ^ diag: none

-- Semantic tokens for expression identifiers
checkBool([[progress == 1 and not active]])
--           ^ tok: variable
--                      ^ tok: number
--                        ^ tok: keyword
--                            ^ tok: keyword
--                                ^ tok: variable

-- expression<self> resolves to receiver's class
---@class SelfExprState
---@field ready boolean
---@field value number
local SelfExprState = {}

---@param expr expression<self>
function SelfExprState:watch(expr) end

local obj = SelfExprState
obj:watch([[ready]])
--           ^ hover: (field) ready: boolean

obj:watch([[missing]])
--           ^ diag: undefined-field

-- Function calls: undeclared callee names are flagged
checkBool([[progress > 0 and unknownFn(count) > 1]])
--           ^ hover: (field) progress: number
--                             ^ diag: undefined-field

-- Intersection type in expression context: fields from both classes available
---@class ExprFuncs
---@field min fun(a: number, b: number): number
---@field max fun(a: number, b: number): number

---@param expr expression<ExprState & ExprFuncs, number>
function checkIntersect(expr) end

-- Declared via intersection: callee is recognized
checkIntersect([[min(progress, count)]])
--                ^ hover: (field) min: fun(a: number, b: number): number  tok: function
--                    ^ hover: (field) progress: number
-- ^ diag: none

-- Hover and def on function from intersected class
checkIntersect([[max(progress, count)]])
--                ^ hover: (field) max: fun(a: number, b: number): number  def: local  tok: function

-- Unknown names still flagged with combined class name
checkIntersect([[badVar + 1]])
--                ^ diag: undefined-field

-- Completions include fields from both classes
checkIntersect([[pro]])
--                ^ comp: progress, active, name, count, min, max

-- expression<self & Funcs> works with intersection
---@class SelfWithFuncs
---@field value number
local SelfWithFuncs = {}

---@class SelfExprFuncs
---@field clamp fun(v: number, lo: number, hi: number): number

---@param expr expression<self & SelfExprFuncs, number>
function SelfWithFuncs:compute(expr) end

local sw = SelfWithFuncs
sw:compute([[clamp(value, 0, 100)]])
--            ^ hover: (field) clamp: fun(v: number, lo: number, hi: number): number
--                   ^ hover: (field) value: number
