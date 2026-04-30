local _, ns = ...

-- ── Class-level generics: type params inherited by colon methods ────────────

-- Path 1: @type on a local with inline fun()
---@type GenericReg<fun(count: number): string>
local reg1 = ns.GenericReg.New()

local out1 = reg1:Invoke("k", 5)
--    ^ hover: (local) out1: string  diag: none

reg1:InvokeAll(5)
--             ^ diag: none

reg1:InvokeAll("wrong")
--             ^ diag: type-mismatch

-- Path 2: @type on a local with alias expanding to fun()
---@type GenericReg<ItemCallback>
local reg2 = ns.GenericReg.New()

reg2:InvokeAll({["a"] = 1})
--             ^ diag: none

reg2:InvokeAll(42)
--             ^ diag: type-mismatch

-- Path 3: table-constructor field with @type
local private = {
    ---@type GenericReg<fun(isReady: boolean)>
    callbacks = ns.GenericReg.New(),
}

private.callbacks:InvokeAll(true)
--                          ^ diag: none

private.callbacks:InvokeAll("wrong")
--                          ^ diag: type-mismatch

-- Path 4: covariant return type in function-type compatibility
---@type GenericReg<fun(name: string): BaseItem>
local reg3 = ns.GenericReg.New()

---@param name string
---@return SpecialItem
local function makeSpecial(name) return ns.SpecialItem end
reg3:Register(makeSpecial)
--            ^ diag: none

---@param name string
---@return string
local function wrongRet(name) return "" end
reg3:Register(wrongRet)
--            ^ diag: type-mismatch

-- Path 5: returns<F> projection cross-file
local item = reg3:Invoke("k", "op")
--    ^ hover: (local) item: BaseItem

-- Path 6: non-generic function returning parameterized callable (concrete type_args)
local iter6 = ns.MakeConcreteIter()
for k6, v6 in iter6 do
    k6 = k6
--  ^ hover: (local) k6: number
    v6 = v6
--  ^ hover: (local) v6: string
end

-- Path 7: non-generic function returning parameterized callable with bare vararg return
local iter7 = ns.MakeVarargIter()
for k7, v7, v7b in iter7 do
    k7 = k7
--  ^ hover: (local) k7: number
    v7 = v7
--  ^ hover: (local) v7: any
    v7b = v7b
--  ^ hover: (local) v7b: any
end

-- Path 8: typed varargs in fun() return type (e.g. ...string)
local iter8 = ns.MakeTypedVarargIter()
for k8, v8, v8b in iter8 do
    k8 = k8
--  ^ hover: (local) k8: number
    v8 = v8
--  ^ hover: (local) v8: string
    v8b = v8b
--  ^ hover: (local) v8b: string
end

-- Path 9: fewer loop variables than returns — only first typed
local iter9 = ns.MakeConcreteIter()
for only9 in iter9 do
    only9 = only9
--  ^ hover: (local) only9: number
end

-- Path 10: chained method call returning parameterized callable
local iter10 = ns.QueryBuilder:Filter("x"):Iterator()
for k10, v10 in iter10 do
    k10 = k10
--  ^ hover: (local) k10: number
    v10 = v10
--  ^ hover: (local) v10: string
end

-- Path 11: colon call returning parameterized callable (method, not static)
---@type Container<fun(): string, boolean>
local container11 = {}
local iter11 = container11:GetIterator()
for k11, v11 in iter11 do
    k11 = k11
--  ^ hover: (local) k11: string
    v11 = v11
--  ^ hover: (local) v11: boolean
end

_G.useGenericClassUser = { reg1, out1, reg2, private, reg3, makeSpecial, wrongRet, item, iter6, iter7 }
