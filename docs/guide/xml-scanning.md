# XML Frame Scanning

wowlua-ls automatically scans `.xml` files in your addon workspace, extracting frame and template definitions so the type system understands XML-defined globals and fields.

## What it does

WoW addon XML files define UI frames, templates, and child elements. Without XML scanning, the language server would report false `undefined-global` diagnostics for frames created in XML and miss field completions for template-defined children.

When wowlua-ls scans an XML file, it extracts:

- **Virtual templates** (`virtual="true"`) become `@class` declarations: available for `@type` annotations and inheritance
- **Non-virtual named frames** become both a class and a global variable: no `undefined-global` warning
- **`parentKey` children** become typed fields on the parent frame's class
- **`KeyValue` elements** become fields with their declared types (string, number, boolean)
- **`inherits` and `mixin`** attributes populate the parent class chain

## Example

Given this XML:

```xml
<Frame name="MyAddonFrame" virtual="true" mixin="MyAddonMixin">
    <Layers>
        <Layer level="BACKGROUND">
            <Texture parentKey="Background" />
        </Layer>
        <Layer level="ARTWORK">
            <FontString parentKey="Title" />
        </Layer>
    </Layers>
    <KeyValues>
        <KeyValue key="label" value="default" type="string" />
        <KeyValue key="count" value="0" type="number" />
    </KeyValues>
</Frame>
```

In your Lua code:

```lua
---@type MyAddonFrame
local frame
frame.Background  -- Texture
frame.Title       -- FontString
frame.label       -- string
frame.count       -- number
```

## Supported features

### Template inheritance

Templates can inherit from other templates via `inherits`. The child gets all parent fields:

```xml
<Button name="MyButton" virtual="true" inherits="MyAddonFrame">
    <HighlightTexture parentKey="Glow" />
</Button>
```

`MyButton` inherits `Background`, `Title`, `label`, and `count` from `MyAddonFrame`, plus its own `Glow` field.

### `$parent` name resolution

Frame names containing `$parent` are resolved using the nearest named parent frame:

```xml
<Frame name="PlayerFrame" parent="UIParent">
    <Frames>
        <Frame name="$parentText" parentKey="Text" />
    </Frames>
</Frame>
```

This creates a global `PlayerFrameText` and a `Text` field on `PlayerFrame`.

### Intrinsic elements

Frames with `intrinsic="true"` define custom XML element types:

```xml
<Button name="ActionButton" intrinsic="true" mixin="ActionButtonMixin" />

<!-- Usage creates a global with the ActionButton type -->
<ActionButton name="MainActionButton" parent="UIParent" />
```

### `parentArray` fields

Child frames with `parentArray` create array-typed fields:

```xml
<Frame name="ListFrame" virtual="true">
    <Frames>
        <Frame parentArray="Items" />
        <Frame parentArray="Items" />
    </Frames>
</Frame>
```

```lua
---@type ListFrame
local list
list.Items  -- Frame[]
```

### Implicit `parentKey`

Special child elements get implicit parentKey names without an explicit attribute:

| Element | Implicit parentKey |
|---|---|
| `NormalTexture` | `NormalTexture` |
| `HighlightTexture` | `HighlightTexture` |
| `PushedTexture` | `PushedTexture` |
| `ThumbTexture` | `ThumbTexture` |
| `ScrollChild` | `ScrollChild` |

### Supported element types

The scanner recognizes all standard WoW frame types:

| Category | Elements |
|---|---|
| Frames | `Frame`, `Button`, `CheckButton`, `EditBox`, `ScrollFrame`, `StatusBar`, `Slider`, `Cooldown`, `GameTooltip`, `MessageFrame`, `Minimap`, `ColorSelect`, `SimpleHTML`, `Browser`, `MovieFrame`, `DropdownButton` |
| Models | `Model`, `ModelScene`, `ModelFFX`, `CinematicModel`, `DressUpModel`, `PlayerModel`, `TabardModel` |
| Textures | `Texture`, `MaskTexture`, and all special texture elements |
| Text | `FontString` and header variants |
| Animation | `AnimationGroup`, `Alpha`, `Scale`, `Translation`, `Rotation` |
| Font | `FontFamily` |

## Limitations

- **Dotted `parentKey` paths**: A `parentKey` like `"IconHitBox.IconBorder"` sets a field on a nested child frame rather than the direct parent. These are silently skipped.
- **`childKey`**: The `childKey` attribute (the inverse of `parentKey` - sets a field on the child pointing back to the parent) is not currently supported.
- **Inline `<Script>` code**: Lua code embedded inside XML `<Script>` blocks is not parsed. Globals defined there (e.g. `STANDARD_TEXT_FONT`) won't be discovered unless they also appear in a `.lua` file.
- **`<FontFamily>` details**: `<FontFamily>` elements create a `Font` class, but their `<Member>` children and font file references are not modeled.

## No configuration needed

XML scanning is enabled by default. Any `.xml` files in your workspace are scanned automatically alongside `.lua` files.
