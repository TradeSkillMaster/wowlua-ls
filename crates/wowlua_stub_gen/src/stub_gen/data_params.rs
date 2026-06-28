//! Retype WoW mixin-object parameters to their underlying *data* type.
//!
//! Several WoW types are modeled (by Ketho's annotations) as a plain data struct
//! plus a behavior mixin, e.g. `colorRGB : ColorRGBData, ColorMixin` and
//! `ItemLocation : ItemLocationData, ItemLocationMixin`. A value *produced* as one
//! of these (a `CreateColor()` result, a struct field) is a real instance with the
//! mixin's methods. But the C APIs that *accept* one only read its data fields in
//! C++ — they never call Lua methods on it — so at runtime a plain `{r,g,b}` /
//! `{bagID,slotIndex}` table works just as well.
//!
//! The analysis engine treats a plain table literal passed as an argument where a
//! class *with methods* is expected as an error (it can't be a real instance — see
//! the require-methods rule in `diagnostics/missing_fields.rs`). That is correct for
//! genuinely-methods-typed parameters, but would false-positive on these data-reading
//! C APIs. So we retype their *parameters* to a `Data | Object` union: a plain data
//! table matches the data member, a real object matches either, and an unannotated
//! caller variable backward-inferred from the argument position keeps a methods-bearing
//! member (so `C_Item.IsBound(loc)` doesn't strip `loc`'s methods). Returns and
//! data-field types are deliberately left alone — a value produced as `colorRGBA` is
//! a real object.

use crate::annotations::{AnnotationType, ClassDecl, ExternalGlobal};
use std::collections::HashMap;

/// Object/mixin type → its plain-data counterpart. Only the types Ketho models with
/// an explicit data/mixin split appear here; every other mixin object type is either
/// method-less (so unaffected) or undefined (permissive) in the stubs.
pub(in crate::stub_gen) const MIXIN_DATA_PARAM_TYPES: &[(&str, &str)] = &[
    ("colorRGB", "ColorRGBData"),
    ("colorRGBA", "ColorRGBAData"),
    ("ItemLocation", "ItemLocationData"),
];

fn data_param_map() -> HashMap<&'static str, &'static str> {
    MIXIN_DATA_PARAM_TYPES.iter().copied().collect()
}

/// Recursively rewrite object-mixin type names to a `Data | Object` union within a
/// *parameter* position: through type wrappers (`?`, `[]`, `T!`, `T & U`,
/// `table<_, T>`, tuples, `{f: T}`) and into nested `fun(...)` parameters — but never
/// a `fun`'s return list.
///
/// The union (rather than just the data type) is the honest signature — the param
/// accepts the plain data struct *or* a full instance — and, crucially, keeps a
/// methods-bearing member: an unannotated caller variable whose type is
/// backward-inferred from this argument position would otherwise become data-only
/// and lose its methods (e.g. `local loc = ...; C_Item.IsBound(loc); loc:IsValid()`).
fn remap_param_type(ty: &mut AnnotationType, map: &HashMap<&'static str, &'static str>) {
    match ty {
        AnnotationType::Simple(name) => {
            if let Some(&data) = map.get(name.as_str()) {
                let object = std::mem::take(name);
                *ty = AnnotationType::Union(vec![
                    AnnotationType::Simple(data.to_string()),
                    AnnotationType::Simple(object),
                ]);
            }
        }
        AnnotationType::Union(v) | AnnotationType::Intersection(v) => {
            for t in v {
                remap_param_type(t, map);
            }
        }
        AnnotationType::Array(b)
        | AnnotationType::NonNil(b)
        | AnnotationType::VarArgs(b)
        | AnnotationType::Backtick(b)
        | AnnotationType::IndexedAccess(_, b) => remap_param_type(b, map),
        AnnotationType::Parameterized(_, args) => {
            for t in args {
                remap_param_type(t, map);
            }
        }
        AnnotationType::TableLiteral(fields) => {
            for (_, t) in fields {
                remap_param_type(t, map);
            }
        }
        // A callback parameter: remap its parameters, not its return types.
        AnnotationType::Fun(params, _returns, _va) => {
            for p in params {
                remap_param_type(&mut p.typ, map);
            }
        }
        AnnotationType::Tuple(positions, _) => {
            for p in positions {
                remap_param_type(&mut p.typ, map);
            }
        }
    }
}

/// Apply the mixin→data parameter remap across all scanned stub globals and class
/// methods. Parameters (including overload and callback parameters) are remapped;
/// returns and data-field values are not.
pub(in crate::stub_gen) fn remap_mixin_data_params(
    globals: &mut [ExternalGlobal],
    classes: &mut [ClassDecl],
) {
    let map = data_param_map();
    for g in globals.iter_mut() {
        for p in &mut g.params {
            remap_param_type(&mut p.typ, &map);
        }
        for ov in &mut g.overloads {
            for p in &mut ov.params {
                remap_param_type(&mut p.typ, &map);
            }
        }
    }
    for c in classes.iter_mut() {
        // Only a method (a `fun`-typed field) is a parameter carrier; a plain
        // data field typed `colorRGB` is a produced value and must not be remapped.
        for (_, ty, _) in &mut c.fields {
            if matches!(ty, AnnotationType::Fun(..)) {
                remap_param_type(ty, &map);
            }
        }
        for ov in &mut c.overloads {
            for p in &mut ov.params {
                remap_param_type(&mut p.typ, &map);
            }
        }
    }
}
