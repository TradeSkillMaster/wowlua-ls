-- A field the workspace scan registers as `any` (its values are `x or Make()` and
-- `other.x`) reads as the writes that resolve type it in the file writing it.
---@class LazyPanel
---@field width number

---@return LazyPanel
local function MakePanel() return { width = 1 } end

---@class LazyHost
local host = {}

function host:Init()
    self.panel = self.panel or MakePanel()
end

function host:Swap(other)
    self.panel = other.panel
end

function host:Width()
    local panel = self.panel
    --    ^ hover: (local) panel: LazyPanel {
    --                 ^ hover: (field) panel: LazyPanel {
    return panel.width
end

-- An author's `any` stays `any` through the same writes.
---@param s string
local function Label(s) return s end

---@class LazyCounter
---@field count any
---@field data any
local counter = {}

function counter:Bump()
    self.count = self.count or 0
end

function counter:Fill()
    self.data = {}
end

function counter:Load(src)
    self.data = src.payload
end

function counter:Text()
    return Label(self.count), Label(self.data)
    --                ^ hover: (field) count: any
    --                                   ^ hover: (field) data: any
end

return host, counter
