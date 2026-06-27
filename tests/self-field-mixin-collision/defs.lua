---@diagnostic disable: unused-local
-- Regression: a *deeply-nested* colon method's `self.field = ...` write must not
-- leak onto an unrelated *top-level* global that happens to share the receiver's
-- leaf name. The descendants-pass mixin handler in scan_globals.rs keys `self` by
-- the receiver's single (leaf) name, so `function MyAddon.B.Collision:Method()`
-- collapses to "Collision" — which collides with the unrelated global `Collision`
-- below. A deeply-nested receiver whose leaf is not a known @class must be
-- skipped, not attached to an arbitrary same-named global.
Collision = {}

MyAddon = {}
MyAddon.B = {}
MyAddon.B.Collision = {}

function MyAddon.B.Collision:Method()
    self.deep = 1
end

-- A single-name mixin (the receiver IS the method's root, so the leaf
-- unambiguously names the table): its self-field MUST still resolve cross-file —
-- this is the plain-global mixin pattern the descendants-pass handler exists for.
PlainMixin = {}
function PlainMixin:Init()
    self.shallow = 1
end

-- A *deeply-nested* receiver whose leaf ("Widget") DOES name a known @class
-- (NestedClass via var_to_class): the handler must re-key the self-field to the
-- class name so it resolves cross-file. The write is nested inside control flow
-- on purpose — that routes it *exclusively* through the descendants pass, since
-- the typed/bare self-field scanners only catch top-level method-body writes (a
-- top-level write here would resolve regardless and would not exercise this leg).
Outer = {}
Outer.Sub = {}
---@class NestedClass
Outer.Sub.Widget = {}

function Outer.Sub.Widget:Configure(flag)
    if flag then
        self.nested = 1
    end
end
