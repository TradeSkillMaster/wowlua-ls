//! WoW game flavor bitmask and helpers.
//!
//! We expose three opinionated flavors matching the folder names Blizzard
//! uses in the WoW install directory:
//!
//!   - `retail` (0x1) — the live retail game
//!   - `classic` (0x2) — the rolling Classic progression, including MoP Classic
//!   - `classic_era` (0x4) — Classic Era (vanilla)
//!
//! Ketho's `vscode-wow-api` repo tracks API availability with a 4-bit mask
//! (mainline / mists / bcc / classic_era). During stub generation we collapse
//! mists and bcc into a single `classic` bit — users who ship a classic addon
//! don't need to distinguish those at the API level.
//!
//! A mask of `0` means "no flavor data known" and is treated as available
//! in all flavors to avoid false positives on unclassified APIs.

pub(crate) const FLAVOR_RETAIL: u8 = 0x1;
pub(crate) const FLAVOR_CLASSIC: u8 = 0x2;
pub(crate) const FLAVOR_CLASSIC_ERA: u8 = 0x4;
pub(crate) const FLAVOR_ALL: u8 = 0x7;

/// Ketho's raw 4-bit flavor mask (mainline / mists / bcc / classic_era)
/// → our 3-bit mask. Mists and bcc both map to our `classic` bit.
pub(crate) fn from_ketho_mask(ketho: u8) -> u8 {
    let mut out = 0u8;
    if ketho & 0x1 != 0 { out |= FLAVOR_RETAIL; }
    if ketho & 0x6 != 0 { out |= FLAVOR_CLASSIC; } // mists | bcc
    if ketho & 0x8 != 0 { out |= FLAVOR_CLASSIC_ERA; }
    out
}

/// Parse a user-provided flavor name to its bitmask bit. Returns `None` for
/// unknown names. Only the three canonical names + `mainline` are accepted —
/// no `wrath` / `cataclysm` / `mop` aliases, since those are all folded into
/// `classic`.
pub(crate) fn parse_flavor_name(name: &str) -> Option<u8> {
    match name.trim().to_ascii_lowercase().as_str() {
        "retail" | "mainline" => Some(FLAVOR_RETAIL),
        "classic" => Some(FLAVOR_CLASSIC),
        "classic_era" => Some(FLAVOR_CLASSIC_ERA),
        _ => None,
    }
}

/// Parse a list of flavor names into a bitmask. Unknown names are ignored.
pub(crate) fn parse_flavor_list(names: &[String]) -> u8 {
    let mut mask = 0u8;
    for n in names {
        if let Some(bit) = parse_flavor_name(n) {
            mask |= bit;
        }
    }
    mask
}

/// WOW_PROJECT_* constant name → flavor bit.
pub(crate) fn wow_project_constant_flavor(name: &str) -> Option<u8> {
    match name {
        "WOW_PROJECT_MAINLINE" => Some(FLAVOR_RETAIL),
        "WOW_PROJECT_CLASSIC" => Some(FLAVOR_CLASSIC_ERA),
        "WOW_PROJECT_BURNING_CRUSADE_CLASSIC"
        | "WOW_PROJECT_WRATH_CLASSIC"
        | "WOW_PROJECT_CATACLYSM_CLASSIC"
        | "WOW_PROJECT_MISTS_CLASSIC" => Some(FLAVOR_CLASSIC),
        _ => None,
    }
}

/// Render a flavor mask as a human-readable list: "Retail, Classic".
pub(crate) fn format_flavor_list(mask: u8) -> String {
    let mut parts = Vec::new();
    if mask & FLAVOR_RETAIL != 0 { parts.push("Retail"); }
    if mask & FLAVOR_CLASSIC != 0 { parts.push("Classic"); }
    if mask & FLAVOR_CLASSIC_ERA != 0 { parts.push("Classic Era"); }
    parts.join(", ")
}

/// Effective flavor mask for diagnostic purposes. A stored `0` (no data) is
/// treated as "available everywhere" so unclassified APIs never trigger
/// `wrong-flavor-api`.
#[inline]
pub(crate) fn effective_mask(mask: u8) -> u8 {
    if mask == 0 { FLAVOR_ALL } else { mask & FLAVOR_ALL }
}

/// Which flavors from `active` are NOT available on the call. Returns the
/// bits where diagnostics should fire. Returns 0 if the call is valid under
/// all active flavors.
pub(crate) fn unsupported_flavors(active: u8, call: u8) -> u8 {
    let call = effective_mask(call);
    active & !call
}

/// Map a TOC filename suffix (without the leading `_`) to a flavor mask.
/// Returns `None` for unrecognized suffixes (e.g. `_Options`).
pub(crate) fn parse_toc_suffix(suffix: &str) -> Option<u8> {
    match suffix {
        "Mainline" | "Standard" => Some(FLAVOR_RETAIL),
        "Classic" => Some(FLAVOR_CLASSIC | FLAVOR_CLASSIC_ERA),
        "Vanilla" => Some(FLAVOR_CLASSIC_ERA),
        "Cata" | "Wrath" | "TBC" | "Mists" => Some(FLAVOR_CLASSIC),
        _ => None,
    }
}

/// Map a WoW game type name (as used in `AllowLoadGameType` and TOC suffixes)
/// to a flavor mask. More permissive than `parse_flavor_name` — accepts
/// expansion-specific names like `cata`, `wrath`, `vanilla`, etc.
pub(crate) fn parse_game_type_name(name: &str) -> Option<u8> {
    match name.trim().to_ascii_lowercase().as_str() {
        "mainline" | "standard" => Some(FLAVOR_RETAIL),
        "classic" => Some(FLAVOR_CLASSIC | FLAVOR_CLASSIC_ERA),
        "vanilla" => Some(FLAVOR_CLASSIC_ERA),
        "cata" | "wrath" | "tbc" | "mists" => Some(FLAVOR_CLASSIC),
        "plunderstorm" | "wowhack" => Some(FLAVOR_RETAIL),
        _ => None,
    }
}

/// Parse a comma-separated list of game type names into a flavor mask.
/// Unknown names are ignored.
pub(crate) fn parse_game_type_list(names: &str) -> u8 {
    let mut mask = 0u8;
    for name in names.split(',') {
        if let Some(bit) = parse_game_type_name(name) {
            mask |= bit;
        }
    }
    mask
}

/// `[Family]` variable expansions: each value and its flavor mask.
pub(crate) const FAMILY_EXPANSIONS: &[(&str, u8)] = &[
    ("Mainline", FLAVOR_RETAIL),
    ("Classic", FLAVOR_CLASSIC | FLAVOR_CLASSIC_ERA),
];

/// `[Game]` variable expansions: each value and its flavor mask.
pub(crate) const GAME_EXPANSIONS: &[(&str, u8)] = &[
    ("Standard", FLAVOR_RETAIL),
    ("Vanilla", FLAVOR_CLASSIC_ERA),
    ("TBC", FLAVOR_CLASSIC),
    ("Wrath", FLAVOR_CLASSIC),
    ("Cata", FLAVOR_CLASSIC),
    ("Mists", FLAVOR_CLASSIC),
];

/// Parse a comma-separated `@flavor-narrows` list into a mask. Strict: returns
/// 0 (no guard applied) if any entry is unknown, so the author is forced to
/// fix the `malformed-annotation` diagnostic rather than getting a silent
/// partial narrowing.
pub(crate) fn parse_flavor_annotation(rest: &str) -> u8 {
    let names: Vec<&str> = rest
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if names.is_empty() { return 0; }
    let mut mask = 0u8;
    for n in &names {
        match parse_flavor_name(n) {
            Some(bit) => mask |= bit,
            None => return 0,
        }
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_canonical_names() {
        assert_eq!(parse_flavor_name("retail"), Some(FLAVOR_RETAIL));
        assert_eq!(parse_flavor_name("mainline"), Some(FLAVOR_RETAIL));
        assert_eq!(parse_flavor_name("Retail"), Some(FLAVOR_RETAIL));
        assert_eq!(parse_flavor_name("classic"), Some(FLAVOR_CLASSIC));
        assert_eq!(parse_flavor_name("classic_era"), Some(FLAVOR_CLASSIC_ERA));
        assert_eq!(parse_flavor_name("bogus"), None);
        // Former aliases are no longer accepted:
        assert_eq!(parse_flavor_name("wrath"), None);
        assert_eq!(parse_flavor_name("mop"), None);
        assert_eq!(parse_flavor_name("cataclysm"), None);
        assert_eq!(parse_flavor_name("vanilla"), None);
    }

    #[test]
    fn ketho_translation() {
        // Ketho 0xF (all four bits) → all three of ours
        assert_eq!(from_ketho_mask(0xF), FLAVOR_ALL);
        // Ketho 0x1 (mainline only) → our retail
        assert_eq!(from_ketho_mask(0x1), FLAVOR_RETAIL);
        // Ketho 0x2 (mists only) → our classic
        assert_eq!(from_ketho_mask(0x2), FLAVOR_CLASSIC);
        // Ketho 0x4 (bcc only) → our classic
        assert_eq!(from_ketho_mask(0x4), FLAVOR_CLASSIC);
        // Ketho 0x6 (mists + bcc) → our classic
        assert_eq!(from_ketho_mask(0x6), FLAVOR_CLASSIC);
        // Ketho 0x8 (classic_era) → our classic_era
        assert_eq!(from_ketho_mask(0x8), FLAVOR_CLASSIC_ERA);
        // Ketho 0xE (everything but retail) → classic + classic_era
        assert_eq!(from_ketho_mask(0xE), FLAVOR_CLASSIC | FLAVOR_CLASSIC_ERA);
    }

    #[test]
    fn zero_mask_treated_as_all() {
        assert_eq!(effective_mask(0), FLAVOR_ALL);
        assert_eq!(effective_mask(FLAVOR_RETAIL), FLAVOR_RETAIL);
    }

    #[test]
    fn unsupported_computes_difference() {
        let diff = unsupported_flavors(FLAVOR_RETAIL | FLAVOR_CLASSIC, FLAVOR_RETAIL);
        assert_eq!(diff, FLAVOR_CLASSIC);
    }

    #[test]
    fn unsupported_respects_no_data() {
        assert_eq!(unsupported_flavors(FLAVOR_ALL, 0), 0);
    }

    #[test]
    fn format_has_readable_names() {
        let s = format_flavor_list(FLAVOR_RETAIL | FLAVOR_CLASSIC);
        assert_eq!(s, "Retail, Classic");
    }

    #[test]
    fn annotation_parsing_is_strict() {
        // All-valid input narrows normally
        assert_eq!(parse_flavor_annotation("retail"), FLAVOR_RETAIL);
        assert_eq!(
            parse_flavor_annotation("retail, classic"),
            FLAVOR_RETAIL | FLAVOR_CLASSIC,
        );
        // Any unknown entry → no guard applied (strict).
        assert_eq!(parse_flavor_annotation("retail, bogus"), 0);
        assert_eq!(parse_flavor_annotation("wrath"), 0); // stale alias
        assert_eq!(parse_flavor_annotation(""), 0);
    }

    #[test]
    fn toc_suffix_mapping() {
        assert_eq!(parse_toc_suffix("Mainline"), Some(FLAVOR_RETAIL));
        assert_eq!(parse_toc_suffix("Standard"), Some(FLAVOR_RETAIL));
        assert_eq!(parse_toc_suffix("Classic"), Some(FLAVOR_CLASSIC | FLAVOR_CLASSIC_ERA));
        assert_eq!(parse_toc_suffix("Vanilla"), Some(FLAVOR_CLASSIC_ERA));
        assert_eq!(parse_toc_suffix("Cata"), Some(FLAVOR_CLASSIC));
        assert_eq!(parse_toc_suffix("Wrath"), Some(FLAVOR_CLASSIC));
        assert_eq!(parse_toc_suffix("TBC"), Some(FLAVOR_CLASSIC));
        assert_eq!(parse_toc_suffix("Mists"), Some(FLAVOR_CLASSIC));
        assert_eq!(parse_toc_suffix("Options"), None);
        assert_eq!(parse_toc_suffix("Config"), None);
    }

    #[test]
    fn game_type_name_mapping() {
        assert_eq!(parse_game_type_name("mainline"), Some(FLAVOR_RETAIL));
        assert_eq!(parse_game_type_name("standard"), Some(FLAVOR_RETAIL));
        assert_eq!(parse_game_type_name("classic"), Some(FLAVOR_CLASSIC | FLAVOR_CLASSIC_ERA));
        assert_eq!(parse_game_type_name("vanilla"), Some(FLAVOR_CLASSIC_ERA));
        assert_eq!(parse_game_type_name("cata"), Some(FLAVOR_CLASSIC));
        assert_eq!(parse_game_type_name("wrath"), Some(FLAVOR_CLASSIC));
        assert_eq!(parse_game_type_name("tbc"), Some(FLAVOR_CLASSIC));
        assert_eq!(parse_game_type_name("mists"), Some(FLAVOR_CLASSIC));
        assert_eq!(parse_game_type_name("plunderstorm"), Some(FLAVOR_RETAIL));
        assert_eq!(parse_game_type_name("wowhack"), Some(FLAVOR_RETAIL));
        assert_eq!(parse_game_type_name("Mainline"), Some(FLAVOR_RETAIL));
        assert_eq!(parse_game_type_name("bogus"), None);
    }

    #[test]
    fn game_type_list_parsing() {
        assert_eq!(parse_game_type_list("mainline, vanilla"), FLAVOR_RETAIL | FLAVOR_CLASSIC_ERA);
        assert_eq!(parse_game_type_list("classic"), FLAVOR_CLASSIC | FLAVOR_CLASSIC_ERA);
        assert_eq!(parse_game_type_list("bogus"), 0);
        assert_eq!(parse_game_type_list("mainline, bogus"), FLAVOR_RETAIL);
    }
}
