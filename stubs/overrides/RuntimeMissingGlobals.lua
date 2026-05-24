---@meta _
-- Runtime globals used by popular addons but not present in
-- BlizzardInterfaceResources or Blizzard's own FrameXML (so they are not
-- discovered by the automatic scan). Add new entries here when running
-- `check` against real addon projects reveals missing globals.

-- LE_* legacy enum constants (numeric).
-- Exact values are not significant; only the type (number) matters for
-- the language server. Values shown are placeholders (0) unless the real
-- value is known.
LE_GAME_ERR_AUCTION_BID_OWN = 3
LE_GAME_ERR_AUCTION_DATABASE_ERROR = 0
LE_GAME_ERR_AUCTION_HIGHER_BID = 0
LE_GAME_ERR_ITEM_NOT_FOUND = 0
LE_GAME_ERR_NOT_ENOUGH_MONEY = 0
LE_GAME_ERR_ITEM_MAX_COUNT = 0
LE_GAME_ERR_TRADE_COMPLETE = 0
LE_ITEM_BIND_ON_ACQUIRE = 1
LE_ITEM_BIND_QUEST = 4
