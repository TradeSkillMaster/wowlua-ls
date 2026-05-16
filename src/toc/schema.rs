/// The kind of value expected for a TOC field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TocValueKind {
    /// Free-form string (Title, Notes, Author, Version).
    FreeText,
    /// Comma-separated list of addon names (Dependencies, OptionalDeps).
    AddonList,
    /// Comma-separated list of variable names (SavedVariables).
    VariableList,
    /// Interface version number(s), comma-separated (e.g. "110002, 11503").
    InterfaceVersion,
    /// Boolean-like: "1"/"0" or "enabled"/"disabled".
    BooleanLike,
    /// Game type list: comma-separated (mainline, cata, classic, etc.).
    GameTypeList,
    /// Global function name (AddonCompartmentFunc).
    FunctionName,
    /// Texture path (IconTexture).
    TexturePath,
}

/// Definition of a known standard TOC field.
#[derive(Debug, Clone)]
pub struct TocFieldDef {
    pub name: &'static str,
    pub doc: &'static str,
    pub required: bool,
    pub value_kind: TocValueKind,
    /// WoW version when the field was introduced (None = original).
    pub since: Option<&'static str>,
    /// Alternative names for this field (e.g. "RequiredDeps" for "Dependencies").
    pub aliases: &'static [&'static str],
}

/// The complete catalog of known standard TOC fields.
pub static TOC_FIELD_CATALOG: &[TocFieldDef] = &[
    TocFieldDef {
        name: "Interface",
        doc: "The WoW client interface version this addon is compatible with. Multiple versions can be comma-separated for multi-flavor TOCs. Examples: `110002` (retail 11.0.2), `11503` (classic 1.15.3).",
        required: true,
        value_kind: TocValueKind::InterfaceVersion,
        since: None,
        aliases: &[],
    },
    TocFieldDef {
        name: "Title",
        doc: "The display name of the addon shown in the addon list.",
        required: false,
        value_kind: TocValueKind::FreeText,
        since: None,
        aliases: &[],
    },
    TocFieldDef {
        name: "Notes",
        doc: "A short description of the addon shown as a tooltip in the addon list.",
        required: false,
        value_kind: TocValueKind::FreeText,
        since: None,
        aliases: &[],
    },
    TocFieldDef {
        name: "Author",
        doc: "The author(s) of the addon.",
        required: false,
        value_kind: TocValueKind::FreeText,
        since: None,
        aliases: &[],
    },
    TocFieldDef {
        name: "Version",
        doc: "The version string of the addon. Free-form (e.g. `1.0.0`, `v2.3`, `@project-version@`).",
        required: false,
        value_kind: TocValueKind::FreeText,
        since: None,
        aliases: &[],
    },
    TocFieldDef {
        name: "SavedVariables",
        doc: "Comma-separated list of global variable names that persist across sessions (account-wide). These globals are saved to `WTF/Account/<name>/SavedVariables/<addon>.lua`.",
        required: false,
        value_kind: TocValueKind::VariableList,
        since: None,
        aliases: &[],
    },
    TocFieldDef {
        name: "SavedVariablesPerCharacter",
        doc: "Comma-separated list of global variable names that persist per-character. These globals are saved to `WTF/Account/<name>/<realm>/<char>/SavedVariables/<addon>.lua`.",
        required: false,
        value_kind: TocValueKind::VariableList,
        since: None,
        aliases: &[],
    },
    TocFieldDef {
        name: "Dependencies",
        doc: "Comma-separated list of addon names that must be loaded before this addon. The addon will not load if any dependency is missing or disabled.",
        required: false,
        value_kind: TocValueKind::AddonList,
        since: None,
        aliases: &["RequiredDeps"],
    },
    TocFieldDef {
        name: "OptionalDeps",
        doc: "Comma-separated list of addon names that should load before this addon if present, but are not required. Used to establish load order when the dependency is optional.",
        required: false,
        value_kind: TocValueKind::AddonList,
        since: None,
        aliases: &[],
    },
    TocFieldDef {
        name: "LoadOnDemand",
        doc: "If set to `1`, the addon is not loaded automatically at login. It must be loaded explicitly via `LoadAddOn()` or by another addon's dependency declaration.",
        required: false,
        value_kind: TocValueKind::BooleanLike,
        since: None,
        aliases: &[],
    },
    TocFieldDef {
        name: "DefaultState",
        doc: "Whether the addon is enabled by default when first installed. Values: `enabled` (default) or `disabled`.",
        required: false,
        value_kind: TocValueKind::BooleanLike,
        since: None,
        aliases: &[],
    },
    TocFieldDef {
        name: "IconTexture",
        doc: "The texture path for the addon's icon, shown in the addon compartment (minimap button menu). Example: `Interface\\Icons\\INV_Misc_QuestionMark`.",
        required: false,
        value_kind: TocValueKind::TexturePath,
        since: Some("10.1.0"),
        aliases: &["IconAtlas"],
    },
    TocFieldDef {
        name: "AddonCompartmentFunc",
        doc: "Global function name called when the addon's compartment button is clicked.",
        required: false,
        value_kind: TocValueKind::FunctionName,
        since: Some("10.1.0"),
        aliases: &[],
    },
    TocFieldDef {
        name: "AddonCompartmentFuncOnEnter",
        doc: "Global function name called when the mouse enters the addon's compartment button (for tooltip display).",
        required: false,
        value_kind: TocValueKind::FunctionName,
        since: Some("10.1.0"),
        aliases: &[],
    },
    TocFieldDef {
        name: "AddonCompartmentFuncOnLeave",
        doc: "Global function name called when the mouse leaves the addon's compartment button.",
        required: false,
        value_kind: TocValueKind::FunctionName,
        since: Some("10.1.0"),
        aliases: &[],
    },
    TocFieldDef {
        name: "AllowLoadGameType",
        doc: "Restricts which game flavors can load this addon. Comma-separated list of: `mainline`, `cata`, `classic`, `wrath`, `mists`. If omitted, the addon loads on all flavors.",
        required: false,
        value_kind: TocValueKind::GameTypeList,
        since: Some("10.2.0"),
        aliases: &[],
    },
    TocFieldDef {
        name: "LoadWith",
        doc: "Comma-separated list of addon names. When *any* listed addon loads, this addon loads too (for LoadOnDemand addons).",
        required: false,
        value_kind: TocValueKind::AddonList,
        since: None,
        aliases: &[],
    },
    TocFieldDef {
        name: "LoadManagers",
        doc: "Comma-separated list of addon names that manage loading this addon. Only one needs to be present for this addon to be loadable.",
        required: false,
        value_kind: TocValueKind::AddonList,
        since: None,
        aliases: &[],
    },
    TocFieldDef {
        name: "OnlyBetaAndPTR",
        doc: "If set to `1`, the addon only loads on Beta and PTR realms.",
        required: false,
        value_kind: TocValueKind::BooleanLike,
        since: None,
        aliases: &[],
    },
    TocFieldDef {
        name: "Secure",
        doc: "If set to `1`, marks the addon as Blizzard-signed secure code. Only applicable to Blizzard's own addons.",
        required: false,
        value_kind: TocValueKind::BooleanLike,
        since: None,
        aliases: &[],
    },
    TocFieldDef {
        name: "Category",
        doc: "The addon category shown in the addon list for organization. Localizable with `Category-<locale>` variants.",
        required: false,
        value_kind: TocValueKind::FreeText,
        since: Some("11.0.2"),
        aliases: &[],
    },
    TocFieldDef {
        name: "Group",
        doc: "Groups related addons together in the addon list under a shared collapsible header.",
        required: false,
        value_kind: TocValueKind::FreeText,
        since: Some("11.0.2"),
        aliases: &[],
    },
    TocFieldDef {
        name: "LoadSavedVariablesFirst",
        doc: "If set to `1`, saved variables are loaded before the addon's Lua files execute (rather than after).",
        required: false,
        value_kind: TocValueKind::BooleanLike,
        since: None,
        aliases: &[],
    },
];

/// Known values for `AllowLoadGameType` and the `[AllowLoadGameType ...]` directive.
pub static GAME_TYPE_VALUES: &[(&str, &str)] = &[
    ("mainline", "Retail (The War Within, etc.)"),
    ("cata", "Cataclysm Classic"),
    ("classic", "Classic Era (Vanilla)"),
    ("wrath", "Wrath of the Lich King Classic"),
    ("mists", "Mists of Pandaria Classic"),
];

/// Known `[Directive]` names for file path lines.
pub static FILE_DIRECTIVES: &[(&str, &str)] = &[
    ("AllowLoadGameType", "Restricts this file to specific game flavors (comma-separated: mainline, cata, classic, wrath, mists)."),
    ("Family", "Path variable that expands to the game family subdirectory."),
    ("Game", "Path variable that expands to the specific game subdirectory."),
];

/// Look up a field by name (case-insensitive) or alias.
pub fn lookup_field(name: &str) -> Option<&'static TocFieldDef> {
    TOC_FIELD_CATALOG.iter().find(|f| {
        f.name.eq_ignore_ascii_case(name)
            || f.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
    })
}

/// Check if a field name is a custom extension field (starts with `X-`).
pub fn is_custom_field(name: &str) -> bool {
    name.starts_with("X-") || name.starts_with("x-")
}

/// Check if a field name is a locale-suffixed field (e.g. `Title-enUS`, `Notes-deDE`).
pub fn is_locale_field(name: &str) -> bool {
    // Standard fields that support locale suffixes
    const LOCALIZABLE: &[&str] = &["Title", "Notes", "Category", "Group"];
    for base in LOCALIZABLE {
        if let Some(suffix) = name.strip_prefix(base)
            && suffix.starts_with('-') && suffix.len() == 5
        {
            return true;
        }
    }
    false
}

/// Map an Interface version number to a human-readable expansion name.
pub fn interface_version_label(version: u32) -> Option<&'static str> {
    match version {
        // Retail: MMMPP format (major * 10000 + minor * 100 + patch)
        120100..=120199 => Some("Midnight 12.1.x"),
        120000..=120099 => Some("Midnight 12.0.x"),
        110105..=110199 => Some("The War Within 11.1.x"),
        110100..=110104 => Some("The War Within 11.1.0"),
        110007..=110099 => Some("The War Within 11.0.x"),
        110000..=110006 => Some("The War Within 11.0.x"),
        100207..=100299 => Some("Dragonflight 10.2.x"),
        100200..=100206 => Some("Dragonflight 10.2.x"),
        100100..=100199 => Some("Dragonflight 10.1.x"),
        100000..=100099 => Some("Dragonflight 10.0.x"),
        90200..=90299 => Some("Shadowlands 9.2.x"),
        90100..=90199 => Some("Shadowlands 9.1.x"),
        90000..=90099 => Some("Shadowlands 9.0.x"),
        80300..=80399 => Some("Battle for Azeroth 8.3.x"),
        80200..=80299 => Some("Battle for Azeroth 8.2.x"),
        80100..=80199 => Some("Battle for Azeroth 8.1.x"),
        80000..=80099 => Some("Battle for Azeroth 8.0.x"),
        70300..=70399 => Some("Legion 7.3.x"),
        70000..=70299 => Some("Legion 7.x"),
        60000..=69999 => Some("Warlords of Draenor 6.x"),
        // Classic Era: 1MMPP
        11500..=11599 => Some("Classic Era 1.15.x"),
        11400..=11499 => Some("Classic Era 1.14.x"),
        11300..=11399 => Some("Classic Era 1.13.x"),
        // Cata Classic: 4MMPP
        40401..=40499 => Some("Cataclysm Classic 4.4.x"),
        40400 => Some("Cataclysm Classic 4.4.0"),
        40300..=40399 => Some("Cataclysm Classic 4.3.x"),
        // Wrath Classic: 3MMPP
        30403..=30499 => Some("Wrath Classic 3.4.x"),
        30400..=30402 => Some("Wrath Classic 3.4.x"),
        // TBC Classic: 2MMPP
        20500..=20599 => Some("TBC Classic 2.5.x"),
        20400..=20499 => Some("TBC Classic 2.4.x"),
        // Mists Classic: 5MMPP
        50500..=50599 => Some("Mists Classic 5.5.x"),
        50400..=50499 => Some("Mists Classic 5.4.x"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_known_field() {
        let field = lookup_field("Interface").unwrap();
        assert_eq!(field.name, "Interface");
        assert!(field.required);
    }

    #[test]
    fn lookup_by_alias() {
        let field = lookup_field("RequiredDeps").unwrap();
        assert_eq!(field.name, "Dependencies");
    }

    #[test]
    fn lookup_case_insensitive() {
        assert!(lookup_field("interface").is_some());
        assert!(lookup_field("TITLE").is_some());
    }

    #[test]
    fn custom_field_detection() {
        assert!(is_custom_field("X-Website"));
        assert!(is_custom_field("x-Curse-Project-ID"));
        assert!(!is_custom_field("Title"));
        assert!(!is_custom_field("Interface"));
    }

    #[test]
    fn locale_field_detection() {
        assert!(is_locale_field("Title-enUS"));
        assert!(is_locale_field("Notes-deDE"));
        assert!(is_locale_field("Category-enUS"));
        assert!(!is_locale_field("Title"));
        assert!(!is_locale_field("X-Title-enUS"));
    }

    #[test]
    fn interface_version_labels() {
        assert_eq!(interface_version_label(110100), Some("The War Within 11.1.0"));
        assert_eq!(interface_version_label(110105), Some("The War Within 11.1.x"));
        assert_eq!(interface_version_label(120001), Some("Midnight 12.0.x"));
        assert_eq!(interface_version_label(100200), Some("Dragonflight 10.2.x"));
        assert_eq!(interface_version_label(11503), Some("Classic Era 1.15.x"));
        assert_eq!(interface_version_label(40400), Some("Cataclysm Classic 4.4.0"));
        assert_eq!(interface_version_label(50503), Some("Mists Classic 5.5.x"));
        assert_eq!(interface_version_label(20505), Some("TBC Classic 2.5.x"));
        assert_eq!(interface_version_label(99999), None);
    }
}
