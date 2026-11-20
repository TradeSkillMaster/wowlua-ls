---@diagnostic disable: unused-local

---@type XfDocRecord
--        ^ doc: A record kept in another file.
--        ^ doc: Spans two lines.
local record = XfDocRecord
--    ^ hover: (local) record: XfDocRecord {\n  Count: fun(): number,\n  Find: fun(id: number): XfDocRecord?,\n  id: number, -- The record's id.\n  note: string?\n}

local id = record.id
--                ^ doc: The record's id.

XfDocRecord.Find(1)
--          ^ doc: @*param* `id` — The id to find.
XfDocRecord.Find(1)
--          ^ doc: @*return* `record` — The record, if any.

-- Doc lines between two declarations in one comment run document the second.
---@type XfDocSecond
--        ^ doc: The second class in the same comment run.
local second = nil
---@type XfDocFirst
--        ^ doc: !The second class
local first = nil

XfDocRecord.Count()
--          ^ doc: @*return* `count` — The count.
XfDocRecord.Count()
--          ^ doc: !The dropped one.
