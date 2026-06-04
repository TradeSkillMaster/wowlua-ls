local _, ns = ...

-- Pool created via generic factory, stored in table constructor field.
-- Backward inference for `task` should pick up XCat from Recycle (via
-- receiver type_args substitution of T → XCat), intersected with XAnimal
-- from RemoveTask, yielding XCat (the more specific child class).
local private = {
    catPool = ns.XPool.New(ns.XCat),
}

function private.FreeCat(task)
    ns.RemoveTask(task)
    private.catPool:Recycle(task)
    --                      ^ hover: (param) task: XCat
end

-- Field assignment variant
local private2 = {}
private2.catPool = ns.XPool.New(ns.XCat)

function private2.FreeCat(task)
    ns.RemoveTask(task)
    private2.catPool:Recycle(task)
    --                       ^ hover: (param) task: XCat
end

_G.usePoolUser = { private, private2 }
