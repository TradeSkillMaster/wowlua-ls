-- Cross-file test: bracket access on @field table<K,V> from another file

---@type XWidgetPool
local pool = {}

-- Direct bracket access on cross-file @field table<K,V>
local w = pool.pool[1]
--    ^ hover: (local) w: XWidget {

-- Through intermediate local
local items = pool.pool
--    ^ hover: (local) items: XWidget[]
local w2 = items[1]
--    ^ hover: (local) w2: XWidget {

-- Method call on bracket-accessed element
pool.pool[1]:GetName()
--           ^ hover: (method) function XWidget:GetName()  def: external

-- Inside a function consuming the pool
---@param p XWidgetPool
local function releaseAll(p)
    for i = 1, p.index do
        local widget = p.pool[i]
        --    ^ hover: (local) widget: XWidget {
        local _ = widget.visible
        --                ^ hover: (field) visible: boolean  def: external
    end
end
