-- Opaque alias: nominally distinct type aliases via @alias (opaque)

---@alias (opaque) PlayerID number
---@alias (opaque) ItemID number
---@alias (opaque) Answer "YES"|"NO"
---@alias (opaque) Toggle "YES"|"NO"

-- ── Hover shows alias name ──────────────────────────────────────────────

---@type PlayerID
local pid = 42
--    ^ hover: (local) pid: PlayerID

---@type ItemID
local iid = 99
--    ^ hover: (local) iid: ItemID

---@type Answer
local ans = "YES"
--    ^ hover: (local) ans: Answer

-- ── Literal assignable to opaque (Rule 2) ───────────────────────────────

---@param id PlayerID
local function lookupPlayer(id) end

lookupPlayer(42)
--           ^ diag: none

---@param a Answer
local function processAnswer(a) end

processAnswer("YES")
--            ^ diag: none

processAnswer("NO")
--            ^ diag: none

-- ── Cross-alias ERROR (the money case) ──────────────────────────────────

lookupPlayer(iid)
--           ^ diag: type-mismatch

---@type Toggle
local tog = "YES"

processAnswer(tog)
--            ^ diag: type-mismatch

-- ── Outward flow OK (opaque → base type) ────────────────────────────────

---@param n number
local function useNumber(n) end

useNumber(pid)
--        ^ diag: none

---@param s string
local function useString(s) end

useString(ans)
--        ^ diag: none

-- ── Same alias OK ───────────────────────────────────────────────────────

---@type PlayerID
local pid2 = 42
lookupPlayer(pid2)
--           ^ diag: none

-- ── Arithmetic decays to base type ──────────────────────────────────────

local sum = pid + 1
--    ^ hover: (local) sum: number

local diff = pid - pid2
--    ^ hover: (local) diff: number

-- ── Opaque in union ─────────────────────────────────────────────────────

---@param x PlayerID|nil
local function maybePlayer(x) end

maybePlayer(nil)
--          ^ diag: none

maybePlayer(42)
--          ^ diag: none

maybePlayer(iid)
--          ^ diag: type-mismatch

-- ── Opaque as return type ───────────────────────────────────────────────

---@return PlayerID
local function createPlayer() return 42 end

local newPid = createPlayer()
--    ^ hover: (local) newPid: PlayerID

lookupPlayer(newPid)
--           ^ diag: none

-- ── Comparison works ────────────────────────────────────────────────────

local cmpResult = pid == 1
--    ^ hover: (local) cmpResult: boolean

local ansCheck = ans == "YES"
--    ^ hover: (local) ansCheck: boolean

-- ── Opaque concatenation (for string-based opaques) ─────────────────────

local greeting = ans .. "!"
--    ^ hover: (local) greeting: string

-- ── Opaque wrapping fun() — hover and cross-alias ────────────────────

---@alias (opaque) Callback fun(x: number): string
---@alias (opaque) OtherCallback fun(x: number): string

---@return Callback
---@diagnostic disable-next-line: missing-return
local function getCallback() end

---@return OtherCallback
---@diagnostic disable-next-line: missing-return
local function getOtherCallback() end

local cb = getCallback()
--    ^ hover: (local) cb: Callback

---@param f Callback
local function invokeCallback(f) end

invokeCallback(cb)
--             ^ diag: none

local otherCb = getOtherCallback()
invokeCallback(otherCb)
--             ^ diag: type-mismatch

-- ── Opaque wrapping table — field access ─────────────────────────────

---@class Config
---@field name string
---@field enabled boolean

---@alias (opaque) AppConfig Config

---@type AppConfig
local cfg = { name = "test", enabled = true }
--    ^ hover: (local) cfg: AppConfig

local cfgName = cfg.name
--    ^ hover: (local) cfgName: string

local cfgEnabled = cfg.enabled
--    ^ hover: (local) cfgEnabled: boolean

-- ── Opaque wrapping table<K,V> — bracket index ──────────────────────

---@alias (opaque) ScoreMap table<string, number>

---@type ScoreMap
local scores = { alice = 100, bob = 200 }

local aliceScore = scores["alice"]
--    ^ hover: (local) aliceScore: number

-- ── Opaque table cross-alias rejection ──────────────────────────────

---@alias (opaque) ServerConfig Config

---@param c AppConfig
local function useAppConfig(c) end

---@type ServerConfig
local srv = { name = "srv", enabled = false }

useAppConfig(srv)
--           ^ diag: type-mismatch

-- ── Nested opaque — field access through multiple layers ─────────────

---@class Pos
---@field x number
---@field y number

---@alias (opaque) WorldPos Pos
---@alias (opaque) ScreenPos WorldPos

---@type ScreenPos
local sp = { x = 10, y = 20 }
--    ^ hover: (local) sp: ScreenPos

local spx = sp.x
--    ^ hover: (local) spx: number

local spy = sp.y
--    ^ hover: (local) spy: number

---@param p WorldPos
local function useWorldPos(p) end

useWorldPos(sp)
--          ^ diag: type-mismatch
