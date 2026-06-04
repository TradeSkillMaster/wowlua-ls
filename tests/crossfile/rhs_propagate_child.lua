---@diagnostic disable: unused-local
-- Child class: overrides parent's any-typed _widget field with a concrete type
local RPParent = RPDefine("RPParent")
local RPChild = RPDefine("RPChild", RPParent)

function RPChild:__init()
    local w = RPCreateWidget()
    self._widget = w
end

function RPChild:DoWork()
    -- _widget should be RPWidget (from child's assignment), not any
    local widget = self._widget
    --     ^ hover: (local) widget: RPWidget {
end

-- Fallback: child also assigns untyped value → extras all resolve to any, stays any
local RPChildAny = RPDefine("RPChildAny", RPParent)

function RPChildAny:__init(thing)
    self._data = thing  -- thing is untyped, so extras are also any
end

function RPChildAny:DoWork()
    -- _data should remain any (no concrete extra_exprs)
    local d = self._data
    --    ^ hover: (local) d: any
end

-- Multiple concrete types: child assigns different types in branches → union
local RPChildMulti = RPDefine("RPChildMulti", RPParent)

function RPChildMulti:__init(flag)
    if flag then
        self._widget = RPCreateWidget()
    else
        self._widget = RPCreateLabel()
    end
end

function RPChildMulti:DoWork()
    -- _widget should be the union of both concrete types
    local w = self._widget
    --    ^ hover: (local) w: RPLabel | RPWidget
end
