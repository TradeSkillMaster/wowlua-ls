---@diagnostic disable: unused-local, unused-function

---@param cb fun(name: string)
local function takesStringCb(cb) end

---@param cb fun(x: number, y: string)
local function takesTwoParams(cb) end

-- Wider actual param (contravariant direction): safe, consumer only passes `string`
---@param name string?
local function widerOptional(name) end
takesStringCb(widerOptional)

---@param name number | string
local function widerUnion(name) end
takesStringCb(widerUnion)

-- Genuine mismatch: wrong param type must still error
---@param name number
local function wrongType(name) end
takesStringCb(wrongType)
--            ^ diag: type-mismatch

-- Exact match: no diagnostic
---@param name string
local function exactMatch(name) end
takesStringCb(exactMatch)

-- Multi-param: wider on second param is safe
---@param x number
---@param y string?
local function widerSecond(x, y) end
takesTwoParams(widerSecond)

-- Multi-param: genuine mismatch on second param
---@param x number
---@param y boolean
local function wrongSecond(x, y) end
takesTwoParams(wrongSecond)
--             ^ diag: type-mismatch

-- Narrower actual param (covariant direction): also accepted (bivariant)
---@param name string
local function narrower(name) end
---@param cb fun(name: string | number)
local function takesWiderCb(cb) end
takesWiderCb(narrower)
