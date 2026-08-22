-- Assigns coarse field4 (undefined call -> `any` placeholder) and reads every
-- sibling coarse field, so analyzing this file drives the cross-file harvest.
function Obj:Set4()
	self.field4 = MakeThing4()
end

function Obj:Use4()
	local _v1 = Obj.field1.member
	local _v2 = Obj.field2.member
	local _v3 = Obj.field3.member
	local _v4 = Obj.field4.member
	local _v5 = Obj.field5.member
	local _v6 = Obj.field6.member
	local _v7 = Obj.field7.member
	local _v8 = Obj.field8.member
end
