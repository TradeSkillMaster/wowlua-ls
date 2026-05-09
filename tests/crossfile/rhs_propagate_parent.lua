-- Parent class: constructor sets self._widget from an untyped parameter (becomes any)
local RPParent = RPDefine("RPParent")

function RPParent:__init(widget)
    self._widget = widget  -- widget is untyped => _widget is any
    self._data = widget    -- also untyped => _data is any
end
