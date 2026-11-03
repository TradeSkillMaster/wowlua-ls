---@diagnostic disable: unused-local
local _, ns = ...

local title = LEDGER_L.TITLE
--    ^ hover: (local) title: string
local empty = LEDGER_L.EMPTY
--    ^ hover: (local) empty: string
local usage = LEDGER_L.USAGE
--    ^ hover: (local) usage: string[]

local store = ns.Store
--    ^ hover: (local) store: LedgerStore
local scale = ns.Options.scale
--    ^ hover: (local) scale: number
local limit = ns.Limit
--    ^ hover: (local) limit: number
local pending = ns.Pending
--    ^ hover: (local) pending: string?
local count = LedgerApp.State.Count
--    ^ hover: (local) count: number

local page = LedgerApp.Internals.Page
--    ^ hover: (local) page: number
local retail = LedgerApp.Internals.Retail
--    ^ hover: (local) retail: boolean
local queue = LedgerApp.Internals.Queue
--    ^ hover: (local) queue: table

local mode = LedgerApp.Window.mode
--    ^ hover: (local) mode: string

local settings = ns.Mode
--    ^ hover: (local) settings: { extra: boolean, scale: number } | { label: string }
local order = ns.Order
--    ^ hover: (local) order: string[]
local first = ns.Order[1]
--    ^ hover: (local) first: string
local layout = LedgerApp.Layout
--    ^ hover: (local) layout: { rows: number, wide: boolean? }
local wide = LedgerApp.Layout.wide
--    ^ hover: (local) wide: boolean?
