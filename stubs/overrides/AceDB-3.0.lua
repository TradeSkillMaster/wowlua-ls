---@meta _
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-db-3-0)

--- AceDB-3.0 manages a saved-variables database with profile support. Retrieved
--- via `LibStub("AceDB-3.0")`.
---@class AceDB-3.0
local AceDB = {}

--- Creates a new database object.
---
--- The shape of the `defaults` table is threaded into the returned object, so the
--- section fields you declared in `defaults` (`profile`, `global`, `char`, …) are
--- typed on the DB — `db.profile.myOption` completes and hovers with its default
--- value's type — while the AceDBObject methods (`SetProfile`,
--- `GetCurrentProfile`, …) stay available on the same object.
---@generic Defaults: AceDB.Schema
---@param tbl string|table The name of the saved-variables global, or the table to use for the database
---@param defaults? Defaults A table of database defaults
---@param defaultProfile? string|true The name of the default profile. If not set, a character-specific profile is used. Pass `true` to use a shared global profile named "Default".
---@return Defaults & AceDBObject-3.0 DB
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-db-3-0#title-2)
function AceDB:New(tbl, defaults, defaultProfile) end

--- The database object returned by `AceDB:New` / `db:RegisterNamespace`. The
--- section fields (`profile`, `global`, `char`, …) are declared as `table` here as
--- a fallback; when the DB is created with typed `defaults`, the specific default
--- shape is threaded in and takes precedence (see `AceDB:New`).
---@class AceDBObject-3.0
---@field char table Character-specific data. Every character has its own database.
---@field realm table Realm-specific data. All of the player's characters on the same realm share this database.
---@field class table Class-specific data. All of the player's characters of the same class share this database.
---@field race table Race-specific data. All of the player's characters of the same race share this database.
---@field faction table Faction-specific data. All of the player's characters of the same faction share this database.
---@field factionrealm table Faction and realm specific data.
---@field factionrealmregion table Faction, realm and region specific data.
---@field locale table Locale-specific data, based on the locale of the game client.
---@field global table Global data. All characters on the same account share this database.
---@field profile table Profile-specific data. All characters using the same profile share this database.
---@field profiles table<string, table> All stored profiles, keyed by profile name.
---@field keys table<string, string> The key used for each database section.
---@field sv table The raw saved-variables table backing this database.
---@field defaults AceDB.Schema Cache of the defaults table.
local DBObjectLib = {}

--- Copies a named profile into the current profile, overwriting any conflicting settings.
---@param name string The name of the profile to be copied into the current profile
---@param silent? boolean If true, do not raise an error when the profile does not exist
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-db-3-0#title-3)
function DBObjectLib:CopyProfile(name, silent) end

--- Deletes a named profile. This profile must not be the active profile.
---@param name string The name of the profile to be deleted
---@param silent? boolean If true, do not raise an error when the profile does not exist
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-db-3-0#title-4)
function DBObjectLib:DeleteProfile(name, silent) end

--- Returns the current profile name used by the database.
---@return string profileName
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-db-3-0#title-5)
function DBObjectLib:GetCurrentProfile() end

--- Returns an already existing namespace from the database object.
---@param name string The name of the existing namespace
---@param silent? boolean If true, silently return nil when the namespace is not found
---@return AceDBObject-3.0? namespace The namespace object if found
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-db-3-0#title-6)
function DBObjectLib:GetNamespace(name, silent) end

--- Returns a table with the names of the existing profiles in the database.
---@param tbl? table A table to reuse to store the profile names in
---@return string[] profiles The names of the existing profiles in the database
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-db-3-0#title-7)
function DBObjectLib:GetProfiles(tbl) end

--- Sets the defaults table for the given database object.
---@param defaults AceDB.Schema A table of defaults for this database
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-db-3-0#title-8)
function DBObjectLib:RegisterDefaults(defaults) end

--- Creates a new database namespace, directly tied to the database. Like the
--- parent DB, the `defaults` shape is threaded into the returned namespace object.
---@generic Defaults: AceDB.Schema
---@param name string The name of the new namespace
---@param defaults? Defaults A table of values to use as defaults
---@return Defaults & AceDBObject-3.0 namespace The created database namespace
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-db-3-0#title-9)
function DBObjectLib:RegisterNamespace(name, defaults) end

--- Resets the entire database, using the string defaultProfile as the new default profile.
---@param defaultProfile? string The profile name to use as the default
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-db-3-0#title-10)
function DBObjectLib:ResetDB(defaultProfile) end

--- Resets the current profile to the default values (if specified).
---@param noChildren? boolean If true, the reset is not populated to the child namespaces of this DB object
---@param noCallbacks? boolean If true, the OnProfileReset callback is not fired
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-db-3-0#title-11)
function DBObjectLib:ResetProfile(noChildren, noCallbacks) end

--- Changes the profile of the database and all of its namespaces to the supplied named profile.
---@param name string The name of the profile to set as the current profile
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-db-3-0#title-12)
function DBObjectLib:SetProfile(name) end

---@param target table The object registering to listen for the callback
---@param eventName AceDB.EventName The name of the event triggering the callback
---@param method? string|function The method to call when the event is fired
---[Documentation](https://www.wowace.com/projects/ace3/pages/ace-db-3-0-tutorial#title-5)
function DBObjectLib.RegisterCallback(target, eventName, method) end

---@param target table The object unregistering the callback
---@param eventName AceDB.EventName The event to unregister
function DBObjectLib.UnregisterCallback(target, eventName) end

---@param target table The object unregistering all of its callbacks
function DBObjectLib.UnregisterAllCallbacks(target) end

---@alias AceDB.EventName
---|"OnProfileChanged"
---|"OnProfileCopied"
---|"OnProfileReset"
---|"OnProfileDeleted"
---|"OnProfileShutdown"
---|"OnNewProfile"
---|"OnDatabaseReset"
---|"OnDatabaseShutdown"

--- The shape accepted for an AceDB `defaults` table: every section is optional, so
--- a `defaults` table may declare any subset of the database sections.
---@class AceDB.Schema
---@field char? table Character-specific data. Every character has its own database.
---@field realm? table Realm-specific data. All of the player's characters on the same realm share this database.
---@field class? table Class-specific data. All of the player's characters of the same class share this database.
---@field race? table Race-specific data. All of the player's characters of the same race share this database.
---@field faction? table Faction-specific data. All of the player's characters of the same faction share this database.
---@field factionrealm? table Faction and realm specific data.
---@field factionrealmregion? table Faction, realm and region specific data.
---@field locale? table Locale-specific data, based on the locale of the game client.
---@field global? table Global data. All characters on the same account share this database.
---@field profile? table Profile-specific data. All characters using the same profile share this database.
