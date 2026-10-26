-- Lists built on a global table and on a static field of a `@class` global, read
-- in another file (reader.lua).
---@diagnostic disable: create-global
NsListsRegistry = {}
NsListsRegistry.handlers = {}

---@param handler fun(event: string)
function NsListsRegistry.Register(handler)
    tinsert(NsListsRegistry.handlers, handler)
end

---@class NsListsQueue
NsListsQueue = {}
NsListsQueue.items = {}

---@param entry NsListsEntry
function NsListsQueue.Push(entry)
    NsListsQueue.items[#NsListsQueue.items + 1] = entry
end

local MODE_A, MODE_B = "a", "b"
NsListsRegistry.defaults = {
    [MODE_A] = false,
    [MODE_B] = 5,
}
