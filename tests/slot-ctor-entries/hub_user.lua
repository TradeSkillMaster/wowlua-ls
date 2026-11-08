---@diagnostic disable: unused-local
local libs = Hub.Libs
--    ^ hover: (local) libs: { Main: HubLib, Missing: any }
local main = Hub.Libs.Main
--    ^ hover: (local) main: HubLib {\n  version: number\n}
local ready = Hub.Flags.ready
--    ^ hover: (local) ready: boolean
local defaults = Hub.Settings.defaults
--    ^ hover: (local) defaults: HubDefaults
local mode = Hub.Mode
--    ^ hover: (local) mode: HubLib {\n  version: number\n}
local legacy = Hub.IsLegacy
--    ^ hover: (local) legacy: boolean
local kind = Hub.Enums.Kind
--    ^ hover: (local) kind: { A: number, B: number }
local a = Hub.Enums.Kind.A
--    ^ hover: (local) a: number
local names = Hub.Enums.Names
--    ^ hover: (local) names: string[]
