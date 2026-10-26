---@diagnostic disable: unused-local, unused-function, missing-return
-- The declarations of `defs.lua`, read from another file: the same types as there.

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

---@type PosIds
local typedIds = {}
local typedId = typedIds[1]
--    ^ hover: (local) typedId: number

---@type PosOptCb
local typedOpt
local typedOptResult = typedOpt(1)
--    ^ hover: (local) typedOptResult: string

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

local mode = PosKinds.Mode
--    ^ hover: (local) mode: PosKinds.Mode {

local chain = f.chain
--    ^ hover: (local) function chain(value: string, next: function)
local nodeValue = f.node.value
--    ^ hover: (local) nodeValue: number
local nextNode = f.node.next
--    ^ hover: (local) nextNode: table?
---@type PosGrow<number>
local grow

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

---@type function
local someFn
local maybeResult = PosMaybeCall(someFn)
--    ^ hover: (local) maybeResult: any?
PosTakesString(maybeResult)

local globalCbResult = PosGlobalCb(1)
--    ^ hover: (local) globalCbResult: string
local globalInlineCbResult = PosGlobalInlineCb(1)
--    ^ hover: (local) globalInlineCbResult: string
local globalId = PosGlobalIds[1]
--    ^ hover: (local) globalId: number
local globalMapped = PosGlobalCbMap["k"](1)
--    ^ hover: (local) globalMapped: string

local topOfFile = PosTopOfFileName("x")
--    ^ hover: (local) topOfFile: string
