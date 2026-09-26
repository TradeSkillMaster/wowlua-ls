---@diagnostic disable: unused-local, create-global

-- A workspace `@returns-enum` factory (the stubs can't know it): exercises the
-- synthesis end to end, independent of the `EnumUtil.MakeEnum` override.
MyEnums = {}

---@param ... string
---@return table
---@returns-enum
function MyEnums.Make(...)
    return tInvert({ ... })
end

-- Read from another file in user.lua.
local _, addon = ...
addon.Anchor = MyEnums.Make("Left", "Right")
---@enum AddonMode
addon.Mode = MyEnums.Make("On", "Off")
GlobalSpeed = MyEnums.Make("Slow", "Fast")

-- Same file: the call is typed as its constructor `{ Small = 1, Large = 2 }`.
local Size = MyEnums.Make("Small", "Large")
--    ^ hover: (local) Size: {\nLarge: number,\nSmall: number\n}
local large = Size.Large
--                 ^ hover: (field) Large: number  def: local 22:36  comp: Large, Small
local typo = Size.Larg
--                ^ diag: undefined-field

-- A dot write adds a member; a dynamic write or passing the table on makes its
-- field set open, as for a constructor.
local Extended = MyEnums.Make("A")
Extended.Extra = 5
local extra = Extended.Extra
local key = "B"
local Dynamic = MyEnums.Make("A")
Dynamic[key] = 2
local b = Dynamic.B
local Passed = MyEnums.Make("A")
print(Passed)
local c = Passed.C

-- Not every argument is a string literal: the declared `table`.
local names = { "A" }
local dynamic = MyEnums.Make(names[1])
--    ^ hover: (local) dynamic: table

-- Exported locals, read in user.lua. The scan guesses that a local built by
-- `F("Name", ...)` is class `Name` (here the widget `Frame`); the enum's members
-- replace that guess, so the export isn't a `field-type-mismatch`.
local Kinds = EnumUtil.MakeEnum("Frame", "Button")
addon.Kinds = Kinds
local Moods = MyEnums.Make("Happy", "Sad")
addon.Moods = Moods

-- A factory on the addon namespace, called through its local name.
---@param ... string
---@return table
---@returns-enum
function addon.MakeEnum(...)
    return tInvert({ ... })
end
---@enum AddonShade
addon.Shade = addon.MakeEnum("Light", "Dark")
