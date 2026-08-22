-- Assigns coarse field5 (undefined call -> `any` placeholder) and reads every
-- sibling coarse field, so analyzing this file drives the cross-file harvest.
function Obj:Set5()
	self.field5 = MakeThing5()
end

function Obj:Use5()
	local _v1 = Obj.field1.member
	local _v2 = Obj.field2.member
	local _v3 = Obj.field3.member
	local _v4 = Obj.field4.member
	local _v5 = Obj.field5.member
	local _v6 = Obj.field6.member
	local _v7 = Obj.field7.member
	local _v8 = Obj.field8.member
end
