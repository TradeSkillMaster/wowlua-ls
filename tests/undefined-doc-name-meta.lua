---@meta

-- Test: annotation type-integrity diagnostics still fire in `@meta` files.
--
-- `@meta` marks a declaration-only stub, so runtime/behavior diagnostics are
-- suppressed there. Annotation type-reference validation is the exception: a
-- reference to a type that doesn't exist (e.g. a `@field`/`@param`/`@alias`
-- naming a removed `@class`/`@alias`) is a real error regardless of file kind.
--
-- Regression: previously `@meta` suppressed ALL diagnostics, so a dead type
-- nested inside another (`table<K, DeadType>`) went unreported indefinitely —
-- the whole point of a type-definition file is defeated silently.

---@alias AchievementID integer
---@alias ContinentID integer
---@enum DetectionGroupStatus

-- The dead type `DetectionGroupStatusValue` (removed) is nested as the value
-- type of a `table<K, V>` on a `@field`. The `table` base and the key alias both
-- resolve, so only the dead value type is flagged.
---@class TrackerDatabase.Profile.Detection
---@field achievementIDs table<AchievementID, DetectionGroupStatusValue>
-- ^ diag: undefined-doc-name
---@field continentIDs table<ContinentID, DetectionGroupStatusValue>
-- ^ diag: undefined-doc-name
---@field good table<AchievementID, DetectionGroupStatus>

-- Undefined names in @param / @return are flagged too.
---@param x MissingParamTypeInMeta
---@return MissingReturnTypeInMeta
local function _f(x) return x end
-- ^ diag: undefined-doc-name
-- ^ diag: undefined-doc-name

-- A known type in the same positions must NOT fire.
---@param y DetectionGroupStatus
---@return AchievementID
local function _g(y) return y end

-- Undefined name nested inside another @alias body is flagged.
---@alias BadDetectionMap table<AchievementID, AnotherDeadType>
-- ^ diag: undefined-doc-name

-- @type on a variable is validated.
---@type MissingVarTypeInMeta
local _v = nil
-- ^ diag: undefined-doc-name

-- Runtime/behavior diagnostics stay suppressed in @meta files: this reference to
-- an undefined global (and the surrounding stub code) would fire in a normal
-- file but must NOT produce any diagnostic here. If the meta scoping regressed,
-- the harness's exhaustive check would fail on an unasserted diagnostic.
local _unused = SomeUndefinedMetaGlobal
