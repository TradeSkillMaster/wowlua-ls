-- A plain global mixin table (no Lua @class — promoted to a class only by the
-- XML `mixin="DataProviderMixin"` reference). Its `self.field = ...` writes in
-- a method body must be tracked so cross-file reads on the mixin class don't
-- false-positive as `undefined-field` / `cannot-call`.
DataProviderMixin = {}

local function MakeHandle() return DataProviderMixin end

function DataProviderMixin:Init(provider)
    self.provider = provider           -- bare value -> existence-only field
    self.onUpdate = function() end       -- function literal -> callable field
    self.handle = MakeHandle():Wrap()    -- chained funcall -> existence-only field
    -- A self-field written inside a nested *named non-colon* function: `self`
    -- closes over Init's receiver just like an anonymous callback does, so
    -- `viaNestedFn` belongs to DataProviderMixin. The descendants `self` handler
    -- must skip this named wrapper and resolve to the enclosing colon method.
    local function configure()
        self.viaNestedFn = true
    end
    configure()
end

function DataProviderMixin:Wrap() return self end
