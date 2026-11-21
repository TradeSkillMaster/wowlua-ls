-- Cross-file test: doc comments of a class, its fields, and a function's
-- params/returns reach hover in another file.

--- A record kept in another file.
--- Spans two lines.
---@class XfDocRecord
---@field id number The record's id.
---@field note? string
XfDocRecord = {}

--- Looks up a record.
---@param id number The id to find.
---@return XfDocRecord? record The record, if any.
function XfDocRecord.Find(id)
    return nil
end

---@class XfDocFirst
---@field a number
--- The second class in the same comment run.
---@class XfDocSecond
---@field b string Second's b.

-- A return whose type doesn't resolve is dropped from the signature; the
-- descriptions of the returns after it stay with their own names.
---@return XfDocNoSuchType missing The dropped one.
---@return number count The count.
function XfDocRecord.Count()
    return nil, 0
end

--- Declared again in the other file, without a doc comment there.
---@class XfDocShared
---@field a number
XfDocShared = {}

-------------------- Banner section --------------------
---@class XfDocBanner
---@field a number
XfDocBanner = {}
