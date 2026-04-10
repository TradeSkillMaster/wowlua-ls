-- SYNTAX TEST "source.lua" "Lua grammar"

-- Keywords and control flow
local x = 5
-- <-- keyword.local.lua
if true then end
--      ^^^^ keyword.control.lua
--           ^^^ keyword.control.lua
for i = 1, 10 do end
-- <-- keyword.control.lua
while true do end
-- <---- keyword.control.lua
repeat until true
-- <---- keyword.control.lua
return nil
-- <---- keyword.control.lua

-- Numbers
local a = 42
--        ^^ constant.numeric.float.lua
local b = 3.14
--        ^^^^ constant.numeric.float.lua
local c = 0xFF
--        ^^^^ constant.numeric.float.hexadecimal.lua

-- Strings
local s1 = "hello"
--         ^^^^^^^ string.quoted.double.lua
local s2 = 'world'
--         ^^^^^^^ string.quoted.single.lua
local s3 = [[multiline]]
--         ^^^^^^^^^^^^^ string.quoted.other.multiline.lua

-- String escapes
local s4 = "tab\there"
--             ^^ constant.character.escape.lua
local s5 = "\x41"
--          ^^^^ constant.character.escape.byte.lua
local s6 = "\u{1F600}"
--          ^^^^^^^^^ constant.character.escape.unicode.lua

-- Constants
local t1 = true
--         ^^^^ constant.language.lua
local t2 = false
--         ^^^^^ constant.language.lua
local t3 = nil
--         ^^^ constant.language.lua
local t4 = ...
--         ^^^ constant.language.lua

-- Self
self.x = 1
-- <-- variable.language.self.lua

-- Logical operators
local r = a and b or not c
--          ^^^ keyword.operator.logical.lua
--                ^^ keyword.operator.logical.lua
--                   ^^^ keyword.operator.logical.lua

-- Arithmetic/comparison operators
local op = 1 + 2
--           ^ keyword.operator.lua
local eq = a == b
--           ^^ keyword.operator.lua
local ne = a ~= b
--           ^^ keyword.operator.lua

-- Built-in functions
print("hello")
-- <--- support.function.lua
tostring(42)
-- <------ support.function.lua
type(x)
-- <-- support.function.lua
pairs({})
-- <--- support.function.lua

-- Library functions
string.find("a", "b")
-- <--------- support.function.library.lua
math.floor(1.5)
-- <-------- support.function.library.lua

-- Comments
-- this is a line comment
-- <-- punctuation.definition.comment.lua
--[[ block comment ]]
-- <-- punctuation.definition.comment.begin.lua

-- Function definition
local function foo(x, y)
--             ^^^ entity.name.function.lua
--                 ^ variable.parameter.function.lua
--                    ^ variable.parameter.function.lua
end

function Bar:method(a)
--       ^^^ entity.name.class.lua
--           ^^^^^^ entity.name.function.lua
--                  ^ variable.parameter.function.lua
end

-- Goto / labels
goto myLabel
-- <-- keyword.control.goto.lua
--   ^^^^^^^ string.tag.lua
::myLabel::
-- <-------- string.tag.lua

-- Annotation: @class
---@class MyClass : Parent
-- ^^^^^^ storage.type.annotation.lua
--        ^^^^^^^ support.class.lua
--                ^ keyword.operator.lua
--                  ^^^^^^ support.class.lua

-- Annotation: @field
---@field myField number
-- ^^^^^^ storage.type.annotation.lua
--        ^^^^^^^ entity.name.variable.lua
--                ^^^^^^ support.type.lua

-- Annotation: @field with visibility
---@field private secret number
-- ^^^^^^ storage.type.annotation.lua
--        ^^^^^^^ storage.modifier.lua
--                ^^^^^^ entity.name.variable.lua
--                       ^^^^^^ support.type.lua

-- Annotation: @field optional
---@field name? string
-- ^^^^^^ storage.type.annotation.lua
--        ^^^^ entity.name.variable.lua
--            ^ keyword.operator.lua
--              ^^^^^^ support.type.lua

-- Annotation: @param with simple type
---@param x number
-- ^^^^^^ storage.type.annotation.lua
--        ^ entity.name.variable.lua
--          ^^^^^^ support.type.lua

-- Annotation: @param with description
---@param name string The name of the thing
-- ^^^^^^ storage.type.annotation.lua
--        ^^^^ entity.name.variable.lua
--             ^^^^^^ support.type.lua
--                    ^^^^^^^^^^^^^^^^^^^^^ comment.block.documentation.lua

-- Annotation: @param optional
---@param x? number
-- ^^^^^^ storage.type.annotation.lua
--        ^ entity.name.variable.lua
--         ^ keyword.operator.lua
--           ^^^^^^ support.type.lua

-- Annotation: @return
---@return boolean
-- ^^^^^^^ storage.type.annotation.lua
--         ^^^^^^^ support.type.lua

---@return ...any
-- ^^^^^^^ storage.type.annotation.lua
--         ^^^ keyword.operator.lua
--            ^^^ support.type.lua

-- Annotation: @type
---@type number
-- ^^^^^ storage.type.annotation.lua
--       ^^^^^^ support.type.lua

-- Annotation: @alias
---@alias MyAlias number|string
-- ^^^^^^ storage.type.annotation.lua
--        ^^^^^^^ variable.lua

-- Annotation: @generic
---@generic T
-- ^^^^^^^^ storage.type.annotation.lua
--          ^ storage.type.generic.lua

-- Annotation: @overload
---@overload fun(x: number): boolean
-- ^^^^^^^^^ storage.type.annotation.lua
--           ^^^ keyword.control.lua

-- Annotation: @overload return:
---@overload return: number, string
-- ^^^^^^^^^ storage.type.annotation.lua
--           ^^^^^^^ keyword.other.unit

-- Annotation: @defclass
---@defclass MyClass : Parent
-- ^^^^^^^^^ storage.type.annotation.lua
--           ^^^^^^^ support.class.lua
--                   ^ keyword.operator.lua
--                     ^^^^^^ support.class.lua

-- Annotation: @builds-field
---@builds-field 1 number
-- ^^^^^^^^^^^^^ storage.type.annotation.lua
--               ^ constant.numeric.integer.lua
--                 ^^^^^^ support.type.lua

-- Annotation: @built-name
---@built-name 2
-- ^^^^^^^^^^^ storage.type.annotation.lua
--             ^ constant.numeric.integer.lua

-- Annotation: @built-extends
---@built-extends
-- ^^^^^^^^^^^^^^ storage.type.annotation.lua

-- Annotation: @diagnostic
---@diagnostic disable: deprecated
-- ^^^^^^^^^^^ storage.type.annotation.lua
--             ^^^^^^^ keyword.other.unit

-- Annotation: @deprecated
---@deprecated
-- ^^^^^^^^^^^ storage.type.annotation.lua

-- Annotation: @nodiscard
---@nodiscard
-- ^^^^^^^^^^ storage.type.annotation.lua

-- Annotation: @meta
---@meta
-- ^^^^^ storage.type.annotation.lua

-- Annotation: @private
---@private
-- ^^^^^^^^ storage.type.annotation.lua

-- Annotation: @protected
---@protected
-- ^^^^^^^^^^ storage.type.annotation.lua

-- Annotation: @cast
---@cast myVar +number
-- ^^^^^ storage.type.annotation.lua
--       ^^^^^ variable.other.lua

-- Annotation: @type-narrows
---@type-narrows MyClass
-- ^^^^^^^^^^^^^^^^^^^^^ storage.type.annotation.lua

-- Annotation: @field index signature
---@field [string] number
-- ^^^^^^ storage.type.annotation.lua
--        ^ keyword.operator.lua
--         ^^^^^^ support.type.lua
--               ^ keyword.operator.lua

-- Documentation comment (3 dashes)
--- This is a doc comment
-- <-- comment.line.double-dash.documentation.lua

-- Regular comment (2 dashes)
-- this is regular
-- <-- comment.line.double-dash.lua

-- Four-dash comment
---- this is four dashes
-- <-- comment.line.double-dash.lua

-- Annotation: @field with non-nil assertion
---@field _db DatabaseTable!
-- ^^^^^^ storage.type.annotation.lua
--        ^^^ entity.name.variable.lua
--            ^^^^^^^^^^^^^ support.type.lua
--                         ^ keyword.operator.lua

-- Annotation: @type with non-nil assertion
---@type DatabaseTable!
-- ^^^^^ storage.type.annotation.lua
--       ^^^^^^^^^^^^^ support.type.lua
--                    ^ keyword.operator.lua

-- Annotation: intersection type
---@type Frame & BackdropTemplate
-- ^^^^^ storage.type.annotation.lua
--       ^^^^^ support.type.lua
--             ^ keyword.operator.lua
--               ^^^^^^^^^^^^^^^^ support.type.lua

-- Annotation: anonymous table literal in @alias
---@alias EncodedData string[]&{compressed: boolean}
-- ^^^^^^ storage.type.annotation.lua
--                            ^ keyword.operator.lua
--                             ^ keyword.operator.lua
--                              ^^^^^^^^^^ support.type.lua
--                                        ^^ keyword.operator.lua
--                                          ^^^^^^^ support.type.lua
--                                                 ^ keyword.operator.lua

-- Annotation: anonymous table literal in @param
---@param opts {name: string, verbose?: boolean}
-- ^^^^^^ storage.type.annotation.lua
--        ^^^^ entity.name.variable.lua
--             ^ keyword.operator.lua
--              ^^^^ support.type.lua
--                  ^^ keyword.operator.lua
--                    ^^^^^^ support.type.lua

-- Annotation: @correlated
---@correlated itemString, duration, buyout
-- ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ storage.type.annotation.lua

-- @param with fun() type
---@param cb fun(x: number): boolean A callback
-- ^^^^^^ storage.type.annotation.lua
--        ^^ entity.name.variable.lua
--           ^^^ keyword.control.lua
