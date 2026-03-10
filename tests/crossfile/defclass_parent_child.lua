-- Cross-file defclass parent test: child class with superclass
local Animal = DefineClassWithParent("Animal")
local Dog = DefineClassWithParent("Dog", Animal)

-- Dog should inherit Animal's methods
Dog:GetSpecies()
-- ^ diag: none

-- __super should be typed as Animal (not generic BaseClass, not nilable)
local sup = Dog.__super
--    ^ hover: (global) sup: Animal {

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
