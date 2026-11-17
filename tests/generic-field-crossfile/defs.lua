-- A generic class's fields, read in another file through receivers that bind
-- its params.

---@class XFieldValue<T>
---@field value T
---@field items T[]

---@class XFieldFixed : XFieldValue<string>

---@class XFieldOptions
---@field option XFieldValue<boolean>
---@field maybe XFieldValue<number>?
