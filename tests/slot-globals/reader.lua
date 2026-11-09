---@diagnostic disable: unused-local
local profile = JOURNAL_PROFILE
--    ^ hover: (local) profile: string
local config = JOURNAL_CONFIG
--    ^ hover: (local) config: {Profiles: {Default: table}, Version: number}?
local cache = JOURNAL_CACHE
--    ^ hover: (local) cache: table<string, number>
local row = JournalRowMixin
--    ^ hover: (local) row: {Init: fun(self: table)} | JournalBaseRowMixin
local scan = JournalApp.Workers.scan
--    ^ hover: (local) scan: JournalWorker
local search = JournalApp.Workers.search
--    ^ hover: (local) search: JournalWorker
local cell = JournalCellMixin
--    ^ hover: (local) cell: {Init: fun(self: table)} | JournalBaseCellMixin
