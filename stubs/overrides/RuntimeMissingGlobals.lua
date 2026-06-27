---@meta _
-- Runtime globals used by popular addons but not present in
-- BlizzardInterfaceResources or Blizzard's own FrameXML (so they are not
-- discovered by the automatic scan). Add new entries here when running
-- `check` against real addon projects reveals missing globals.

-- Frame globals created via CreateFrame() at runtime in FrameXML Lua code.
-- These cannot be discovered by XML scanning (which only sees declared frames)
-- or by Lua source scanning (which doesn't interpret CreateFrame call results).

---@type Frame
ObjectiveTrackerBlocksFrame = nil

-- Font objects declared in Interface/FrameXML/Fonts.xml (classic FrameXML).
-- The Gethe/wow-ui-source repository ships Interface/AddOns/ (Blizzard addon
-- code) but NOT Interface/FrameXML/ (core client UI). Fonts.xml lives in the
-- latter, so Font globals like this one are not auto-discovered by the scanner.

---@type Font
SystemFont_NamePlate_Outlined = nil

-- AuctionFrame sub-frames defined in Interface/FrameXML/AuctionFrame.xml
-- (classic FrameXML). Same source gap as above — the wow-ui-source clone only
-- contains AddOns/, so these classic-client globals are not auto-discovered.
--
-- Note: these globals exist only in the classic game client. Ideally they would
-- carry a flavor restriction so retail projects could be warned when they
-- reference them. However, the flavor system (`wrong-flavor-api` diagnostic)
-- only fires on *function calls*, not on reads of global variables. Until
-- `undefined-global` gains flavor-aware filtering, the restriction cannot be
-- expressed as a stub annotation.

---@type Frame
AuctionFrameTop = nil
---@type Frame
AuctionFrameTopLeft = nil
---@type Frame
AuctionFrameTopRight = nil
---@type Frame
AuctionFrameBot = nil
---@type Frame
AuctionFrameBotLeft = nil
---@type Frame
AuctionFrameBotRight = nil

-- Utility functions defined in Interface/FrameXML/ Lua (classic FrameXML).
-- Not discoverable from wow-ui-source (which only ships Interface/AddOns/).

---@type fun(pool: table, frame: Frame)
FramePool_HideAndClearAnchors = nil

-- Boolean-like global variables set in Interface/FrameXML/ Lua (classic).
-- WoW stores these as "1"/nil (truthy string or nil), so string|nil is the
-- most accurate type, but number works in practice because addon code only
-- checks them for truthiness.

---@type string|nil
ENABLE_COLORBLIND_MODE = nil

-- Table globals populated at runtime from FrameXML Lua (classic).
-- Same flavor-restriction caveat as AuctionFrame* above.

---@type table
ACHIEVEMENTUI_CATEGORIES = nil

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
-- Pet journal filter not present in current LuaEnum.lua but referenced by addons.
LE_PET_JOURNAL_FILTER_FAVORITES = 0

-- Legacy bare-global API functions that are absent from EVERY machine-readable
-- discovery source the stub generator scans:
--   * not in BlizzardInterfaceResources GlobalAPI.lua / FrameXML.lua (those list
--     only the modern C_* namespaced forms, in all branches),
--   * not in Blizzard's APIDocumentationGenerated, and
--   * with no standalone `API <Name>` page in the wiki function categories.
-- Wiki-documented legacy bare globals (GetContainerItemInfo, IsAddOnLoaded, the
-- rest of the GetContainerItem*/AddOn families, …) ARE recovered automatically by
-- the wiki-category discovery; only these undiscoverable stragglers need listing.
-- They default to FLAVOR_ALL, so addons that branch between the bare and C_* forms
-- per flavor never see a false `undefined-global` or `wrong-flavor-api`.

-- Deprecated alias of C_ChatInfo.RegisterAddonMessagePrefix. The wiki keeps only a
-- redirect for the bare name (it is not a category member), so the category-member
-- scan never surfaces it. Addons select it on the non-retail path (e.g. AceComm-3.0
-- calls C_ChatInfo.RegisterAddonMessagePrefix on retail and the bare form otherwise).
---@param prefix string
---@return Enum.RegisterAddonMessagePrefixResult result
function RegisterAddonMessagePrefix(prefix) end

-- Reagent-bank API (added in Warlords of Draenor, removed from retail in 11.2.0):
-- bare C globals that were never present in APIDocumentationGenerated and have no
-- wiki API page. Bank/inventory addons still reference them on the code paths where
-- the reagent bank exists (guarded by a per-flavor / feature check at the call site).
---@return number cost
function GetReagentBankCost() end

function BuyReagentBank() end

---@return boolean
function IsReagentBankUnlocked() end
