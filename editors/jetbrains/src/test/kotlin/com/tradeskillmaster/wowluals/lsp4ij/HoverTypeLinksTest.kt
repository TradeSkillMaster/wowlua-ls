package com.tradeskillmaster.wowluals.lsp4ij

import com.tradeskillmaster.wowluals.lsp4ij.HoverTypeLinks.TypeLink
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * The parsing and linking behind clickable hover types, against the hover shape
 * the server sends (`lua_hover` in its `handlers.rs`).
 */
class HoverTypeLinksTest {
    private val signature = "```wowlua-hover\n(local) rec: Record {\n  range: Range\n}\n```"

    @Test
    fun splitsTheGoToLineOffTheHover() {
        val markdown = "$signature\n---\nA record.\n\n---\n\n" +
            "Go to: [`Range`](file:///a/My%20%28Addon%29/defs.lua#L3) · [`Lib-2.0`](file:///a/lib.lua#L10)"
        val (rest, links) = HoverTypeLinks.splitFooter(markdown)
        assertEquals("$signature\n---\nA record.", rest)
        assertEquals(
            listOf(
                TypeLink("Range", "file:///a/My%20%28Addon%29/defs.lua#L3"),
                TypeLink("Lib-2.0", "file:///a/lib.lua#L10"),
            ),
            links,
        )
    }

    @Test
    fun leavesAHoverWithoutAGoToLineWhole() {
        assertEquals(signature to emptyList<TypeLink>(), HoverTypeLinks.splitFooter(signature))
        // A doc that ends in something else after a rule isn't a link line.
        val doc = "$signature\n---\nNotes.\n\n---\n\nGo to: the [manual](https://example.com) for more."
        assertEquals(doc to emptyList<TypeLink>(), HoverTypeLinks.splitFooter(doc))
    }

    /** The same inputs and words as the server's `signature_type_words` tests. */
    @Test
    fun findsTheWordsTheServerLinks() {
        fun words(text: String) = HoverTypeLinks.typeWordRanges(text).map { text.substring(it) }
        val sig = "(class) Record {\n  Range: Range[]?, -- Deprecated, use Other\n  cb: fun(r: Range, mode?: Mode): Record,\n" +
            "  kind: \"Kind\" | 'Kind2' | `T`,\n  count: 0xFF | -1 | 1.5,\n  map: table<string, Lib-2.0.Column>\n}"
        assertEquals(
            listOf("class", "Record", "Range", "Range", "Mode", "Record", "table", "string", "Lib-2.0.Column"),
            words(sig),
        )
        assertEquals(
            listOf("method", "function", "Frame", "FramePoint", "Frame"),
            words("(method) function Frame:SetPoint(point: FramePoint)\n  -> ...Frame"),
        )
        assertEquals(listOf("field", "Frame"), words("(field) x: Frame"))
        assertEquals(emptyList<String>(), words("Name:"))
        assertEquals(listOf("local", "function", "A"), words("(local) function Make(a: A)"))
        assertEquals(listOf("Bar"), words("x: \"a\\\"Foo\" | Bar"))
        assertEquals(emptyList<String>(), words("x: \"Foo"))
        assertEquals(listOf("Bar"), words("x: \"Foo\ny: Bar"))
        assertEquals(
            listOf("local", "function", "Frame", "Result", "Other"),
            words("(local) function Skin(frame, button: Frame, size)\n  -> Result, Other"),
        )
        assertEquals(
            listOf("Helper", "table", "string", "Entry", "Result", "Extra"),
            words("  Apply: fun(self: Helper, frame, map: table<string, Entry>): Result, Extra,"),
        )
        assertEquals(listOf("enum", "Mode"), words("(enum) Mode {\n  Line = 3,\n  Frame = 4\n}"))
        assertEquals(listOf("local", "Mode"), words("(local) mode: Mode = \"fast\""))
        assertEquals(listOf("alias", "Mode", "Kind"), words("(alias) Mode = Kind"))
    }

    @Test
    fun linksTypesInsideTheHighlightedSignature() {
        // Shaped like LSP4IJ's highlighted code block: colored spans, escaped text.
        val html = "<pre><span style=\"color:#a\">Range</span>: <span>Range</span>&lt;<span>Lib</span>-<span>2.0</span>&gt;" +
            " -- <span>Range</span>\n  kind: &quot;Range&quot;</pre><p>See Range.</p><pre>Range</pre>"
        val linked = HoverTypeLinks.linkify(html, mapOf("Range" to "wowlua-type:R?in=a&at=b", "Lib-2.0" to "L"))
        assertEquals(
            "<pre><span style=\"color:#a\">Range</span>: <span><a href=\"wowlua-type:R?in=a&amp;at=b\">Range</a></span>&lt;" +
                "<span><a href=\"L\">Lib</a></span><a href=\"L\">-</a><span><a href=\"L\">2.0</a></span>&gt;" +
                " -- <span>Range</span>\n  kind: &quot;Range&quot;</pre><p>See Range.</p><pre>Range</pre>",
            linked,
        )
    }

    @Test
    fun readsALineBreakTagAsTheEndOfAComment() {
        val html = "<pre>a: B -- B<br/>c: B</pre>"
        assertEquals(
            "<pre>a: <a href=\"h\">B</a> -- B<br/>c: <a href=\"h\">B</a></pre>",
            HoverTypeLinks.linkify(html, mapOf("B" to "h")),
        )
    }

    @Test
    fun aLinkCarriesItsTypeContextAndDeclaration() {
        val link = TypeLink("Lib-2.0.Column", "file:///a/My%20%28Addon%29/defs.lua#L3")
        val href = HoverTypeLinks.href(link, "1a2b", "file:///a/My%20Addon/user.lua")
        assertEquals(
            HoverTypeLinks.LinkTarget("Lib-2.0.Column", "1a2b", "file:///a/My%20Addon/user.lua", link.declarationUrl),
            HoverTypeLinks.parseHref(href),
        )
        assertNull(HoverTypeLinks.parseHref("file:///a/defs.lua#L3"))
        assertEquals("file:///a/My%20%28Addon%29/defs.lua" to 2, HoverTypeLinks.declarationLine(link.declarationUrl))
        assertNull(HoverTypeLinks.declarationLine("file:///a/defs.lua"))
    }
}
