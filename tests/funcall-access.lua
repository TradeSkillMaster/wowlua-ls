-- Tests for dot/bracket access on function call return values

---@class FuncResult
---@field name string
---@field value number
---@field nested FuncNested

---@class FuncNested
---@field deep string

---@class FuncChain
---@field GetResult fun(self: FuncChain): FuncResult

---@return FuncResult
local function getResult()
    return { name = "test", value = 42, nested = { deep = "hello" } }
end

---@return FuncChain
local function getChain()
    return {}
end

-- Basic dot access on function call return
local x = getResult().name
--                     ^ hover: name: string

local y = getResult().value
--                     ^ hover: value: number

-- Chained dot access: func().field.subfield
local z = getResult().nested.deep
--                            ^ hover: deep: string

-- Colon method call on function return, then dot access on its return
local w = getChain():GetResult().name
--                                ^ hover: name: string

-- Hover on intermediate field in chained access
local w2 = getResult().nested
--                      ^ hover: nested: FuncNested

-- Method access on function return via colon
local a = getChain():GetResult()
--                    ^ hover: GetResult: fun(self: FuncChain): FuncResult

-- Inheritance: method returns parent class with fields
---@class FuncBase
---@field id number

---@class FuncChild : FuncBase
---@field label string
---@field GetChild fun(self: FuncChild): FuncChild

---@return FuncChild
local function getChild()
    return {}
end

-- Access inherited field on function return
local b = getChild().id
--                    ^ hover: id: number

-- Access own field on function return
local c = getChild().label
--                    ^ hover: label: string

-- Chained method call: func():method().field
local d = getChild():GetChild().label
--                               ^ hover: label: string
