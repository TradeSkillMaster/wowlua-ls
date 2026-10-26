---@diagnostic disable: unused-local, unused-function, missing-return, create-global
-- One annotation, every position: each read below resolves to the same type
-- whether the type is written inline or through an alias, and whether it sits on
-- a field, an optional field, inside a container, in a class's `table<K, V>`
-- parent, on a local, a parameter, a return or a callback's parameters. This file
-- reads its own declarations; `user.lua` reads them from another file.

---@alias PosCb fun(x: number): string
---@alias PosOptCb (fun(x: number): string)?
---@alias PosCbList PosCb[]
---@alias PosIds integer[]
---@alias PosIdMap table<string, integer>
---@alias PosShape {a: integer}
-- An alias naming one declared after it, here and in another file (`z_late.lua`).
---@alias PosEarlyHandler fun(e: PosLateShape)
---@alias PosLateShape {n: integer}
---@alias PosAllEvents PosLateEvent | "early"

---@class PosFields
---@field cb PosCb
---@field cbInline fun(x: number): string
---@field cbLate PosCb!
---@field optCb PosOptCb
---@field list PosCb[]
---@field listInline (fun(x: number): string)[]
---@field optList? PosCb[]
---@field optListInline? (fun(x: number): string)[]
---@field map table<string, PosCb[]>
---@field optMap? table<string, PosCb[]>
---@field mapInline table<string, (fun(x: number): string)[]>
---@field aliasList PosCbList
---@field ids PosIds
---@field idMap PosIdMap
---@field shape PosShape
---@field nested fun(context: any): (fun(): string?)
---@field chain PosChain<string>
---@field node PosNode<number>
---@field dict PosDict<string, number>
---@field typedMap PosTypedMap<string, number>
---@field dictList PosDict<string, number>[]
PosFields = {}

---@class PosCbMap : table<string, PosCb>
PosCbMap = {}

---@class PosInlineCbMap : table<string, fun(x: number): string>
PosInlineCbMap = {}

---@class PosBox<T>
---@field value T
---@field list T[]
---@field cb fun(v: T): T
PosBox = {}

---@enum PosKinds.Mode
PosKinds.Mode = { Off = 0, On = 1 }

-- A generic class whose method signature takes the class itself.
---@class PosOwner<T>
---@field state T
---@field handler fun(owner: PosOwner, state: T)
PosOwner = {}

---@type PosFields
local f = {}
local cbResult = f.cb(1)
--    ^ hover: (local) cbResult: string
local cbInlineResult = f.cbInline(1)
--    ^ hover: (local) cbInlineResult: string
local cbLateResult = f.cbLate(1)
--    ^ hover: (local) cbLateResult: string
local optCbResult = f.optCb(1)
--    ^ hover: (local) optCbResult: string
local listResult = f.list[1](1)
--    ^ hover: (local) listResult: string
local listInlineResult = f.listInline[1](1)
--    ^ hover: (local) listInlineResult: string
local optListResult = f.optList[1](1)
--    ^ hover: (local) optListResult: string
local optListInlineResult = f.optListInline[1](1)
--    ^ hover: (local) optListInlineResult: string
local mapResult = f.map["k"][1](1)
--    ^ hover: (local) mapResult: string
local optMapResult = f.optMap["k"][1](1)
--    ^ hover: (local) optMapResult: string
local mapInlineResult = f.mapInline["k"][1](1)
--    ^ hover: (local) mapInlineResult: string
local aliasListResult = f.aliasList[1](1)
--    ^ hover: (local) aliasListResult: string
local id = f.ids[1]
--    ^ hover: (local) id: number
local mapped = f.idMap["k"]
--    ^ hover: (local) mapped: number
local shapeA = f.shape.a
--    ^ hover: (local) shapeA: number
local optList = f.optList
--    ^ hover: (local) optList: (fun(x: number): string)[]?
local nestedResult = f.nested(1)()
--    ^ hover: (local) nestedResult: string?

---@type PosCbMap
local cbMap = {}
local fromParent = cbMap["k"](1)
--    ^ hover: (local) fromParent: string

---@type PosInlineCbMap
local inlineCbMap = {}
local fromInlineParent = inlineCbMap["k"](1)
--    ^ hover: (local) fromInlineParent: string

---@type PosCb[]?
local typedList
local typedResult = typedList[1](1)
--    ^ hover: (local) typedResult: string

---@type PosOptCb
local typedOpt
local typedOptResult = typedOpt(1)
--    ^ hover: (local) typedOptResult: string

---@param cb PosCb
---@param list PosCbList
local function takes(cb, list)
    local fromParam = cb(1)
    --    ^ hover: (local) fromParam: string
    local fromParamList = list[1](1)
    --    ^ hover: (local) fromParamList: string
end

---@return PosCb[]
local function gives() end
local fromReturn = gives()[1](1)
--    ^ hover: (local) fromReturn: string

---@param run fun(items: integer[], byName: table<string, PosCb>)
local function withItems(run) end
withItems(function(items, byName)
    local firstItem = items[1]
    --    ^ hover: (local) firstItem: number
    local named = byName["k"](1)
    --    ^ hover: (local) named: string
end)

---@param run PosEarlyHandler
local function withLate(run) end
withLate(function(e)
    local lateN = e.n
    --    ^ hover: (local) lateN: number
end)

---@param event PosAllEvents
local function fire(event) end
fire("late")
fire("early")

---@generic T
---@param v T
---@return PosBox<T>
local function box(v) end
local boxed = box(5)
local boxedValue = boxed.value
--    ^ hover: (local) boxedValue: number
local boxedItem = boxed.list[1]
--    ^ hover: (local) boxedItem: number
local boxedCb = boxed.cb(1)
--    ^ hover: (local) boxedCb: number

---@generic T
---@param s T
---@return PosOwner<T>
local function makeOwner(s) end
local ownerState = makeOwner(5).state
--    ^ hover: (local) ownerState: number

-- A redeclaration naming itself keeps the alias it redeclares.
---@alias PosSelfCb fun(x: number): string
---@alias PosSelfCb PosSelfCb
-- ^ diag: duplicate-doc-alias
---@class PosSelfHolder
---@field cb PosSelfCb
---@type PosSelfHolder
local selfHolder = {}
local selfResult = selfHolder.cb(1)
--    ^ hover: (local) selfResult: string

-- A callable class's `@overload` is not this file's scope: its parameter names
-- are not in scope here.
---@class PosCallable
---@overload fun(posOverloadParam: number): string
local PosCallable = {}
local leaked = posOverloadParam
--             ^ diag: undefined-global

-- A recursive parameterized alias names itself inside its own expansion, where
-- it is a plain `function` or `table`.
---@alias PosChain<T> fun(value: T, next: PosChain<T>)
---@alias PosNode<T> {value: T, next: PosNode<T>?}
---@alias PosGrow<T> fun(next: PosGrow<T[]>)
local chain = f.chain
--    ^ hover: (local) function chain(value: string, next: function)
local nodeValue = f.node.value
--    ^ hover: (local) nodeValue: number
local nextNode = f.node.next
--    ^ hover: (local) nextNode: table?
---@type PosGrow<number>
local grow

-- A generic class declaring itself a map of its type params (a `table<K, V>`
-- parent or a `[K] V` field), instantiated: its reads are typed by the arguments.
---@class PosDict<K, V> : table<K, V>
PosDict = {}
---@return integer
function PosDict:Size() return 0 end

---@class PosTypedMap<K, V>
---@field [K] V
PosTypedMap = {}

---@alias PosNumMap PosTypedMap<string, number>
---@alias PosNumDict PosDict<string, number>

---@class PosScores : PosDict<string, number>
PosScores = {}

---@param s string
function PosTakesString(s) end

local dictValue = f.dict["k"]
--    ^ hover: (local) dictValue: number
local dictSize = f.dict:Size()
--    ^ hover: (local) dictSize: number
local typedMapValue = f.typedMap["k"]
--    ^ hover: (local) typedMapValue: number
local dictListValue = f.dictList[1]["k"]
--    ^ hover: (local) dictListValue: number
---@type PosDict<string, number>
local dict = {}
local typedDictValue = dict["k"]
--    ^ hover: (local) typedDictValue: number
---@type PosNumMap
local numMap = {}
local numMapValue = numMap["k"]
--    ^ hover: (local) numMapValue: number
---@type PosNumDict
local numDict = {}
local numDictValue = numDict["k"]
--    ^ hover: (local) numDictValue: number
---@type PosScores
local scores = {}
local scoresValue = scores["k"]
--    ^ hover: (local) scoresValue: number
PosTakesString(f.dict["k"])
--             ^ diag: type-mismatch
PosTakesString(numMap["k"])
--             ^ diag: type-mismatch

-- `returns<F>` beside `nil` is `F`'s returns, or `any` while `F` is unbound.
---@generic F
---@param fn F
---@return returns<F>?
function PosMaybeCall(fn) end
---@type function
local someFn
local maybeResult = PosMaybeCall(someFn)
--    ^ hover: (local) maybeResult: any?
PosTakesString(maybeResult)

-- A global's `---@type`.
---@type PosCb
PosGlobalCb = nil
---@type fun(x: number): string
PosGlobalInlineCb = nil
---@type integer[]
PosGlobalIds = nil
---@type table<string, PosCb>
PosGlobalCbMap = {}
