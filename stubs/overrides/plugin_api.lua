---@meta
-- Type declarations for the wowlua-ls diagnostic plugin API.
-- These types are available globally so plugin files get hover, completion,
-- and signature help when edited with wowlua-ls.

--- A byte-offset range in the source file.
---@class wowlua.plugin.Range
---@field start integer Byte offset of the range start (0-based)
---@field end integer Byte offset of the range end (exclusive, 0-based)

---------------------------------------------------------------------------
-- FileContext — the main object passed to a plugin's run(ctx) function
---------------------------------------------------------------------------

--- The file context passed to a plugin's `run` function.
--- Provides methods to query analysis results and emit diagnostics.
---@class wowlua.plugin.FileContext
---@field uri string Full file URI (e.g. "file:///path/to/file.lua")
---@field file_name string File basename (e.g. "Module.lua")
local FileContext = {}

--- Find local variables declared at file scope.
---
--- The optional filter table narrows results:
--- - `name`: only return variables with this exact name.
--- - `init`: only return variables whose initializer matches this kind
---   (`"table"`, `"call"`, or `"function"`).
---@param opts? {name?: string, init?: "table"|"call"|"function"}
---@return wowlua.plugin.LocalVar[]
function FileContext:find_locals(opts) end

--- Find `@event` declarations from the workspace.
---
--- Returns all event declarations aggregated across the workspace. Events from
--- stub files, scanned addon files, and the current file are all included.
--- Use `source_uri` to identify which file each event was declared in.
---@param type_name? string Only return events of this type (e.g. `"WowEvent"`)
---@return wowlua.plugin.EventDecl[]
function FileContext:find_event_declarations(type_name) end

--- Emit a **warning** diagnostic at the given source range.
---@param range wowlua.plugin.Range Source range to underline
---@param message string Diagnostic message shown to the user
function FileContext:warn(range, message) end

--- Emit a **hint** diagnostic at the given source range.
---@param range wowlua.plugin.Range Source range to underline
---@param message string Diagnostic message shown to the user
function FileContext:hint(range, message) end

--- Emit an **error** diagnostic at the given source range.
---@param range wowlua.plugin.Range Source range to underline
---@param message string Diagnostic message shown to the user
function FileContext:error(range, message) end

--- Emit an **information** diagnostic at the given source range.
---@param range wowlua.plugin.Range Source range to underline
---@param message string Diagnostic message shown to the user
function FileContext:info(range, message) end

---------------------------------------------------------------------------
-- LocalVar — a file-scope local variable
---------------------------------------------------------------------------

--- A local variable declared at file scope.
---@class wowlua.plugin.LocalVar
---@field name string The variable name
---@field range wowlua.plugin.Range Byte range of the variable's definition site
---@field init? wowlua.plugin.Initializer The variable's initializer expression (nil if none)
local LocalVar = {}

--- Get all field read accesses on this variable (e.g. `var.field` or `var.field()`).
---@return wowlua.plugin.FieldAccess[]
function LocalVar:field_reads() end

--- Get all field write accesses on this variable (e.g. `var.field = value`).
---@return wowlua.plugin.FieldAccess[]
function LocalVar:field_writes() end

--- Get all method/function calls on this variable (both `var:method(args)` and `var.func(args)`).
---@return wowlua.plugin.MethodCall[]
function LocalVar:method_calls() end

--- Get all method/function definitions on this variable (both `function var:method() end` and `function var.func() end`).
---@return wowlua.plugin.MethodDef[]
function LocalVar:method_defs() end

---------------------------------------------------------------------------
-- Initializer — the right-hand side of a local declaration
---------------------------------------------------------------------------

--- The initializer expression of a local variable declaration.
--- For `local x = {}`, this represents the `{}` table constructor.
--- For `local x = Foo:Bar(args)`, this represents the call expression.
---@class wowlua.plugin.Initializer
---@field kind "table"|"call"|"function"|"literal"|"other" The kind of initializer expression
local Initializer = {}

--- Get the fields of a table constructor initializer.
--- Returns an empty table if the initializer is not a table constructor.
---@return wowlua.plugin.FieldInfo[]
function Initializer:fields() end

--- Get the receiver name of a call initializer (e.g. `"Foo"` in `Foo:Bar()`).
--- Returns nil if the initializer is not a call or has no identifiable receiver.
---@return string?
function Initializer:receiver() end

--- Get the method/function name of a call initializer (e.g. `"Bar"` in `Foo:Bar()`
--- or `Foo.Bar()`). Returns nil if the initializer is not a call.
---@return string?
function Initializer:method() end

--- Get the arguments of a call initializer.
--- Returns an empty table if the initializer is not a call.
---@return wowlua.plugin.ArgInfo[]
function Initializer:args() end

---------------------------------------------------------------------------
-- FieldInfo — a field from a table constructor
---------------------------------------------------------------------------

--- A field declared in a table constructor (e.g. `{name = value}`).
---@class wowlua.plugin.FieldInfo
---@field name string The field name
---@field range wowlua.plugin.Range Byte range of the field definition
---@field value_kind "nil"|"function"|"table"|"number"|"string"|"boolean"|"expr" The kind of the field's value expression

---------------------------------------------------------------------------
-- FieldAccess — a read or write of a field on a variable
---------------------------------------------------------------------------

--- A field access (read or write) on a variable.
--- Represents expressions like `var.fieldName` or assignments like `var.fieldName = value`.
---@class wowlua.plugin.FieldAccess
---@field field_name string The accessed field name
---@field range wowlua.plugin.Range Byte range of the access expression

---------------------------------------------------------------------------
-- MethodCall — a method or function call on a variable
---------------------------------------------------------------------------

--- A method or function call on a variable (e.g. `var:methodName(arg1, arg2)` or `var.funcName(arg1)`).
---@class wowlua.plugin.MethodCall
---@field method_name string The called method name
---@field range wowlua.plugin.Range Byte range of the call expression
local MethodCall = {}

--- Get the arguments passed to this method call.
---@return wowlua.plugin.ArgInfo[]
function MethodCall:args() end

---------------------------------------------------------------------------
-- MethodDef — a method definition on a variable
---------------------------------------------------------------------------

--- A method or function definition on a variable (e.g. `function var:methodName() end` or `function var.funcName() end`).
---@class wowlua.plugin.MethodDef
---@field method_name string The defined method name
---@field range wowlua.plugin.Range Byte range of the definition
local MethodDef = {}

--- Get the parameters of this method definition (excluding implicit `self`).
---@return wowlua.plugin.Param[]
function MethodDef:params() end

---------------------------------------------------------------------------
-- Param — a function parameter
---------------------------------------------------------------------------

--- A parameter of a function/method definition.
---@class wowlua.plugin.Param
---@field name string The parameter name
---@field index integer The 1-based parameter index
local Param = {}

--- Find equality comparisons (`==` or `~=`) involving this parameter.
--- Useful for detecting dispatch patterns like `if action == "buy" then`.
---@return wowlua.plugin.ComparisonInfo[]
function Param:comparisons() end

---------------------------------------------------------------------------
-- ArgInfo — an argument in a function/method call
---------------------------------------------------------------------------

--- An argument passed to a function or method call.
---@class wowlua.plugin.ArgInfo
---@field range wowlua.plugin.Range Byte range of the argument expression
---@field kind "string"|"number"|"boolean"|"nil"|"table"|"function"|"other" The kind of the argument expression
---@field literal? string|number|boolean The literal value if the argument is a constant (nil if not a literal)

---------------------------------------------------------------------------
-- ComparisonInfo — an equality comparison involving a symbol
---------------------------------------------------------------------------

--- An equality comparison (`==` or `~=`) involving a variable or parameter.
---@class wowlua.plugin.ComparisonInfo
---@field range wowlua.plugin.Range Byte range of the comparison expression
---@field literal? string|number|boolean The literal value being compared against (nil if not a literal)

---------------------------------------------------------------------------
-- EventDecl — an @event declaration from the workspace
---------------------------------------------------------------------------

--- An event declaration from an `@event TypeName "EVENT_NAME"` annotation.
--- Aggregated across the entire workspace (all scanned files and stubs).
---@class wowlua.plugin.EventDecl
---@field type_name string The event type (e.g. "WowEvent", "FrameEvent")
---@field event_name string The event name (e.g. "ENCOUNTER_END", "OnLoad")
---@field params wowlua.plugin.EventParam[] The event's payload parameters
---@field range? wowlua.plugin.Range Byte range of the declaration in the source file (nil for built-in stubs)
---@field source_uri? string File URI where this event was declared (nil for built-in stubs)

---------------------------------------------------------------------------
-- EventParam — a parameter of an @event declaration
---------------------------------------------------------------------------

--- A parameter declared in an `@event` annotation's payload.
---@class wowlua.plugin.EventParam
---@field name string Parameter name (e.g. "encounterID")
---@field type_name string Type name (e.g. "number", "string")
---@field nilable boolean Whether the parameter is optional
---@field description? string Human-readable description of the parameter

---------------------------------------------------------------------------
-- Plugin return table
---------------------------------------------------------------------------

--- The table returned by a diagnostic plugin file.
---
--- Example:
--- ```lua
--- return {
---   code = "my-check",
---   run = function(ctx)
---     for _, var in ipairs(ctx:find_locals({init = "table"})) do
---       -- analyze var...
---     end
---   end,
--- }
--- ```
---@class wowlua.plugin.Plugin
---@field code string Unique diagnostic code (e.g. "my-check"). Used in `@diagnostic disable: my-check`.
---@field run fun(ctx: wowlua.plugin.FileContext) The analysis function called once per file.
