-- Cross-file inferred return type: a no-@return method returns a class-typed
-- field access. The coarse workspace scan maps such returns to `any`; the lazy
-- whole-file resolver should recover the precise class type for cross-file
-- callers (matching the definition-site engine inference).

---@class InfThing
---@field id number
local InfThing = {}

---@class InfRepo
---@field _items table<string, InfThing?>
local InfRepo = {}

-- No @return annotation: the engine infers the value type of the typed field
-- access (`InfThing?`) from the body.
function InfRepo:GetItem(key)
    return self._items[key]
end
