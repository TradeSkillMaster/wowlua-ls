local _, ns = ...

-- ── Class-level generics: type params inherited by colon methods ────────────

-- Path 1: @type on a local with inline fun()
---@type GenericReg<fun(count: number): string>
local reg1 = ns.GenericReg.New()

local out1 = reg1:Invoke("k", 5)
--    ^ hover: (global) out1: string  diag: none

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
--    ^ hover: (global) item: BaseItem

_G.useGenericClassUser = { reg1, out1, reg2, private, reg3, makeSpecial, wrongRet, item }
