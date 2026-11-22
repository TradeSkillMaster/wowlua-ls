package com.tradeskillmaster.wowluals

import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.editor.colors.EditorColorsManager
import com.intellij.openapi.editor.colors.EditorColorsScheme
import com.intellij.openapi.editor.colors.impl.AbstractColorsScheme
import com.intellij.openapi.editor.ex.EditorEx
import com.intellij.openapi.editor.markup.TextAttributes
import com.intellij.openapi.util.Disposer
import com.intellij.testFramework.ExtensionTestUtil
import com.intellij.testFramework.PlatformTestUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import com.tradeskillmaster.wowluals.lsp4ij.WowLuaLanguageServerFactory
import org.eclipse.lsp4j.MarkupContent
import org.eclipse.lsp4j.MarkupKind
import org.jetbrains.plugins.textmate.TextMateService
import org.jetbrains.plugins.textmate.api.TextMateBundleProvider
import java.awt.Color
import java.nio.file.Path

/**
 * A comment's delimiters (`--`, `---`, `--[[`, `]]`) color as the comment, in
 * the editor and in hovers, under a color scheme that gives punctuation a color
 * of its own.
 *
 * IntelliJ layers the colors of all of a token's TextMate scopes and maps
 * `punctuation` to the "dot" color, which the New UI "Dark" and "Island Dark"
 * schemes set: with the delimiters scoped `punctuation.definition.comment.lua`,
 * as the VS Code grammar scopes them, they showed in that color beside
 * comment-grey text. `sync_grammars.py` drops that scope from the vendored
 * grammar this test loads.
 */
class CommentDelimiterColorTest : BasePlatformTestCase() {
    private val dotColor = Color(0xBC, 0xBE, 0xC4)

    override fun setUp() {
        super.setUp()
        // Disposed in reverse: the bundle goes away, then the reload restores the
        // bundles the other tests run with.
        Disposer.register(testRootDisposable) { loadBundles() }
        val vendoredLua = object : TextMateBundleProvider {
            override fun getBundles() =
                listOf(TextMateBundleProvider.PluginBundle("Lua", Path.of("textmate/lua").toAbsolutePath()))
        }
        ExtensionTestUtil.addExtensions(TextMateBundleProvider.EP_NAME, listOf(vendoredLua), testRootDisposable)
        loadBundles()

        val colors = EditorColorsManager.getInstance()
        val previous = colors.globalScheme
        Disposer.register(testRootDisposable) { colors.setGlobalScheme(previous) }
        val scheme = (colors.getScheme("Darcula") as AbstractColorsScheme).clone() as EditorColorsScheme
        scheme.setAttributes(DefaultLanguageHighlighterColors.DOT, TextAttributes(dotColor, null, null, null, 0))
        colors.setGlobalScheme(scheme)
    }

    fun testEditorColorsCommentDelimitersAsTheComment() {
        myFixture.configureByText("delimiters.lua", "---@class Foo\nlocal x = 1 -- note\n--[[ block ]]\n")
        val commentColor = EditorColorsManager.getInstance().globalScheme
            .getAttributes(DefaultLanguageHighlighterColors.LINE_COMMENT).foregroundColor
        val text = myFixture.editor.document.charsSequence
        val colors = mutableMapOf<String, Color?>()
        val tokens = (myFixture.editor as EditorEx).highlighter.createIterator(0)
        while (!tokens.atEnd()) {
            colors[text.subSequence(tokens.start, tokens.end).toString()] = tokens.textAttributes?.foregroundColor
            tokens.advance()
        }
        for (delimiter in listOf("---", "--", "--[[", " block ]]")) {
            assertEquals("`$delimiter` in $colors", commentColor, colors[delimiter])
        }
    }

    fun testHoverColorsAFieldDescriptionsDelimiterAsTheComment() {
        val file = myFixture.configureByText("hover.lua", "local x = 1\n")
        val feature = WowLuaLanguageServerFactory().createClientFeatures().hoverFeature
        val html = feature.getContent(
            MarkupContent(MarkupKind.MARKDOWN, "```wowlua-hover\n(class) R {\n  a: number, -- A description.\n}\n```"),
            file,
        )!!
        val dotHex = "#%02x%02x%02x".format(dotColor.red, dotColor.green, dotColor.blue)
        assertTrue("the delimiter is highlighted: $html", html.contains("--"))
        assertFalse("no text takes the punctuation color: $html", html.contains(dotHex, ignoreCase = true))
    }

    private fun loadBundles() {
        val textMate = TextMateService.getInstance()
        textMate.reloadEnabledBundles()
        // The reload registers the bundles in the background.
        val deadline = System.currentTimeMillis() + 20_000
        while (textMate.getLanguageDescriptorByExtension("lua") == null && System.currentTimeMillis() < deadline) {
            PlatformTestUtil.dispatchAllEventsInIdeEventQueue()
            Thread.sleep(20)
        }
    }
}
