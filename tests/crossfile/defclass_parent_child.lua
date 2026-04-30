-- Cross-file defclass parent test: child class with superclass
local Animal = DefineClassWithParent("Animal")
local Dog = DefineClassWithParent("Dog", Animal)

-- Dog should inherit Animal's methods
Dog:GetSpecies()
-- ^ diag: none

-- __super should be typed as Animal (not generic BaseClass, not nilable)
local sup = Dog.__super
--    ^ hover: (local) sup: Animal {

-- Inherited method via __super should resolve
Dog.__super:GetSpecies()
--          ^ hover: (method) function Animal:GetSpecies()  def: external

-- Protected methods from BaseClass should be accessible at file scope
-- when the variable was created via @defclass in this file
Dog:OnModuleLoad(function() end)
-- ^ diag: none

-- Classes without a parent should not have a specific __super
-- (they still get the BaseClass constraint's fields but not Animal-specific ones)
local Cat = DefineClassWithParent("Cat")
Cat:baseMethod()
-- ^ diag: none

-- Same for classes without a specific parent
Cat:OnModuleLoad(function() end)
-- ^ diag: none

-- self.__super inside a method should be non-nil, no need-check-nil
function Dog:Bark()
    self.__super:GetSpecies()
    --           ^ hover: (method) function Animal:GetSpecies()  def: external  diag: none
end

-- __super call to a method with partial @param annotations should not
-- produce redundant-parameter warnings for the unannotated params
function Dog:SortStuff()
    self.__super:GetSortValue("a", "col", true)
    --           ^ hover: (method) function Animal:GetSortValue(row: string, id, isAscending)  def: external  diag: none
end

-- Compact @defclass T:P syntax (no space around colon) should also work
local Poodle = CompactDefine("Poodle", Animal)
local poodleSup = Poodle.__super
--    ^ hover: (local) poodleSup: Animal {
function Poodle:Yip()
    self.__super:GetSpecies()
    --           ^ hover: (method) function Animal:GetSpecies()  def: external  diag: none
end

-- Explicitly passing nil as parent should not trigger generic-constraint-mismatch
local Fish = DefineClassWithParent("Fish", nil)
--                                        ^ diag: none
Fish:baseMethod()
-- ^ diag: none

-- Backtick-wrapped parent param should also resolve __super correctly
local Beagle = BacktickDefine("Beagle", Animal)
local beagleSup = Beagle.__super
--    ^ hover: (local) beagleSup: Animal {
function Beagle:Woof()
    self.__super:GetSpecies()
    --           ^ hover: (method) function Animal:GetSpecies()  def: external  diag: none
end
