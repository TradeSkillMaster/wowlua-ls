//! WoW game flavor bitmask and helpers.
//!
//! We expose three opinionated flavors matching the folder names Blizzard
//! uses in the WoW install directory:
//!
//!   - `retail` (0x1) — the live retail game
//!   - `classic` (0x2) — the rolling Classic progression, including MoP Classic
//!   - `classic_era` (0x4) — Classic Era (vanilla)
//!
//! During stub generation, flavor bitmasks are derived from API presence across
//! BlizzardInterfaceResources branches (live / classic / classic_era).
//!
//! A mask of `0` means "no flavor data known" and is treated as available
//! in all flavors to avoid false positives on unclassified APIs.

pub const FLAVOR_RETAIL: u8 = 0x1;
pub const FLAVOR_CLASSIC: u8 = 0x2;
pub const FLAVOR_CLASSIC_ERA: u8 = 0x4;
pub const FLAVOR_ALL: u8 = 0x7;

/// Parse a user-provided flavor name to its bitmask bit. Returns `None` for
/// unknown names. Only the three canonical names + `mainline` are accepted —
/// no `wrath` / `cataclysm` / `mop` aliases, since those are all folded into
/// `classic`.
pub fn parse_flavor_name(name: &str) -> Option<u8> {
    match name.trim().to_ascii_lowercase().as_str() {
        "retail" | "mainline" => Some(FLAVOR_RETAIL),
        "classic" => Some(FLAVOR_CLASSIC),
        "classic_era" => Some(FLAVOR_CLASSIC_ERA),
        _ => None,
    }
}

/// Parse a list of flavor names into a bitmask. Unknown names are ignored.
pub fn parse_flavor_list(names: &[String]) -> u8 {
    let mut mask = 0u8;
    for n in names {
        if let Some(bit) = parse_flavor_name(n) {
            mask |= bit;
        }
    }
    mask
}

/// WOW_PROJECT_* constant name → flavor bit.
pub fn wow_project_constant_flavor(name: &str) -> Option<u8> {
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
pub fn format_flavor_list(mask: u8) -> String {
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
pub fn effective_mask(mask: u8) -> u8 {
    if mask == 0 { FLAVOR_ALL } else { mask & FLAVOR_ALL }
}

/// Which flavors from `active` are NOT available on the call. Returns the
/// bits where diagnostics should fire. Returns 0 if the call is valid under
/// all active flavors.
pub fn unsupported_flavors(active: u8, call: u8) -> u8 {
    let call = effective_mask(call);
    active & !call
}

/// The flavor(s) in which WoW API deprecations originate. `@deprecated` marks
/// flow from the retail/mainline API surface (Ketho's `Annotations/Core`
/// stubs); the same bare API frequently remains the live, non-deprecated form
/// on Classic / Classic Era (e.g. `GetMerchantItemInfo` is the current API on
/// Classic, replaced only on retail by `C_MerchantFrame.GetItemInfo`). So a
/// deprecation is treated as applying to retail only. If per-flavor deprecation
/// data ever becomes available, narrow this per function instead.
pub const DEPRECATION_ORIGIN_FLAVORS: u8 = FLAVOR_RETAIL;

/// Decide whether a `@deprecated` warning should be suppressed for a call,
/// given the addon's declared flavor set (`addon_flavors`) and the function's
/// availability mask (`fn_flavors`).
///
/// Returns true when the function is still **live** — available, and outside
/// the deprecation-origin flavor (retail) — in at least one flavor the addon
/// targets. There the bare API is the correct, non-deprecated form, so flagging
/// it as deprecated is a false positive.
///
/// `addon_flavors == 0` means the addon declares no flavors at all (no config
/// and no `.toc`); suppression is disabled so existing behavior is preserved.
pub fn deprecation_suppressed(addon_flavors: u8, fn_flavors: u8) -> bool {
    if addon_flavors == 0 {
        return false;
    }
    let available = effective_mask(fn_flavors);
    addon_flavors & available & !DEPRECATION_ORIGIN_FLAVORS != 0
}

/// Map a TOC `## Interface:` version number (e.g. `120005`, `50503`, `11508`)
/// to a flavor mask by its major version. WoW interface numbers are
/// `MAJOR*10000 + MINOR*100 + PATCH`, and the major version distinguishes the
/// game flavor: `1.x` is Classic Era (vanilla); the `2.x`–`5.x` re-releases
/// (TBC / Wrath / Cata / MoP Classic) are the rolling Classic; `6.x` and above
/// is retail (currently `11.x` / `12.x`).
pub fn interface_number_flavor(n: u32) -> u8 {
    match n / 10000 {
        1 => FLAVOR_CLASSIC_ERA,
        2..=5 => FLAVOR_CLASSIC,
        _ => FLAVOR_RETAIL,
    }
}

/// Parse a TOC `## Interface:` header value — one or more comma-separated
/// version numbers — into a unioned flavor mask. A multi-version line like
/// `120005, 50503, 11508` yields Retail | Classic | Classic Era. Returns 0 if
/// no number parses.
pub fn parse_interface_flavors(value: &str) -> u8 {
    let mut mask = 0u8;
    for part in value.split(',') {
        if let Ok(n) = part.trim().parse::<u32>() {
            mask |= interface_number_flavor(n);
        }
    }
    mask
}

/// Map a TOC filename suffix (without the leading `_`) to a flavor mask.
/// Returns `None` for unrecognized suffixes (e.g. `_Options`).
pub fn parse_toc_suffix(suffix: &str) -> Option<u8> {
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
pub fn parse_game_type_name(name: &str) -> Option<u8> {
    match name.trim().to_ascii_lowercase().as_str() {
        "mainline" | "standard" => Some(FLAVOR_RETAIL),
        "classic" => Some(FLAVOR_CLASSIC | FLAVOR_CLASSIC_ERA),
        "vanilla" => Some(FLAVOR_CLASSIC_ERA),
        "cata" | "wrath" | "tbc" | "mists" => Some(FLAVOR_CLASSIC),
        "plunderstorm" | "wowhack" => Some(FLAVOR_RETAIL),
        _ => None,
    }
}

/// Is `name` a TOC inline *path variable* (`[Family]`, `[Game]`, `[TextLocale]`)
/// rather than a load *condition* (`[AllowLoadGameType ...]`, `[AllowLoadTextLocale
/// ...]`, `[AllowLoad ...]`)? Path variables are part of the file path and expand
/// per flavor/locale; conditions are separate tokens that restrict when a file
/// loads. Used to decide which brackets to strip vs. keep when parsing a file line.
pub fn is_toc_path_variable(name: &str) -> bool {
    matches!(name, "Family" | "Game" | "TextLocale")
}

/// Parse a comma-separated list of game type names into a flavor mask.
/// Unknown names are ignored.
pub fn parse_game_type_list(names: &str) -> u8 {
    let mut mask = 0u8;
    for name in names.split(',') {
        if let Some(bit) = parse_game_type_name(name) {
            mask |= bit;
        }
    }
    mask
}

/// `[Family]` variable expansions: each value and its flavor mask.
pub const FAMILY_EXPANSIONS: &[(&str, u8)] = &[
    ("Mainline", FLAVOR_RETAIL),
    ("Classic", FLAVOR_CLASSIC | FLAVOR_CLASSIC_ERA),
];

/// `[Game]` variable expansions: each value and its flavor mask.
pub const GAME_EXPANSIONS: &[(&str, u8)] = &[
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
pub fn parse_flavor_annotation(rest: &str) -> u8 {
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

    #[test]
    fn interface_number_to_flavor() {
        // Classic Era is 1.x; the 2.x–5.x re-releases are rolling Classic.
        assert_eq!(interface_number_flavor(11508), FLAVOR_CLASSIC_ERA); // 1.15.8
        assert_eq!(interface_number_flavor(20505), FLAVOR_CLASSIC);     // 2.5.5 (TBC)
        assert_eq!(interface_number_flavor(30403), FLAVOR_CLASSIC);     // 3.x (Wrath)
        assert_eq!(interface_number_flavor(40400), FLAVOR_CLASSIC);     // 4.x (Cata)
        assert_eq!(interface_number_flavor(50503), FLAVOR_CLASSIC);     // 5.5.3 (MoP)
        // Retail is 6.x and above (currently 11.x / 12.x).
        assert_eq!(interface_number_flavor(110005), FLAVOR_RETAIL);     // 11.0.5
        assert_eq!(interface_number_flavor(120005), FLAVOR_RETAIL);     // 12.0.5
    }

    #[test]
    fn interface_header_flavor_union() {
        assert_eq!(parse_interface_flavors("11508"), FLAVOR_CLASSIC_ERA);
        assert_eq!(parse_interface_flavors("120005"), FLAVOR_RETAIL);
        // A multi-version line (the Auctionator / UtilityHub shape).
        assert_eq!(
            parse_interface_flavors("120005, 50503, 11508"),
            FLAVOR_RETAIL | FLAVOR_CLASSIC | FLAVOR_CLASSIC_ERA,
        );
        assert_eq!(parse_interface_flavors("20505, 11508"), FLAVOR_CLASSIC | FLAVOR_CLASSIC_ERA);
        // Whitespace and garbage tolerated; no number → 0.
        assert_eq!(parse_interface_flavors("  120005  "), FLAVOR_RETAIL);
        assert_eq!(parse_interface_flavors(""), 0);
        assert_eq!(parse_interface_flavors("abc"), 0);
    }

    #[test]
    fn deprecation_suppression_logic() {
        // A function deprecated on retail but live on Classic / Classic Era
        // (e.g. GetMerchantItemInfo, available = Classic | Classic Era).
        let classic_only = FLAVOR_CLASSIC | FLAVOR_CLASSIC_ERA;
        // Addon targets only retail → not live anywhere else it targets → warn.
        assert!(!deprecation_suppressed(FLAVOR_RETAIL, classic_only));
        // Addon targets Classic Era → live there → suppress.
        assert!(deprecation_suppressed(FLAVOR_CLASSIC_ERA, classic_only));
        // Multi-flavor addon (retail + era) → still live on era → suppress.
        assert!(deprecation_suppressed(FLAVOR_RETAIL | FLAVOR_CLASSIC_ERA, classic_only));

        // A function available everywhere (mask 0 / FLAVOR_ALL), deprecated on
        // retail (e.g. GetItemInfo).
        assert!(!deprecation_suppressed(FLAVOR_RETAIL, 0));            // retail addon → warn
        assert!(deprecation_suppressed(FLAVOR_CLASSIC_ERA, 0));        // classic addon → suppress
        assert!(deprecation_suppressed(FLAVOR_RETAIL | FLAVOR_CLASSIC, FLAVOR_ALL)); // multi → suppress

        // No declared flavors at all → suppression disabled (prior behavior).
        assert!(!deprecation_suppressed(0, classic_only));
        assert!(!deprecation_suppressed(0, 0));

        // A genuinely retail-only deprecated function is always flagged (it is
        // not live outside retail anywhere).
        assert!(!deprecation_suppressed(FLAVOR_CLASSIC_ERA, FLAVOR_RETAIL));
        assert!(!deprecation_suppressed(FLAVOR_RETAIL, FLAVOR_RETAIL));
    }
}
