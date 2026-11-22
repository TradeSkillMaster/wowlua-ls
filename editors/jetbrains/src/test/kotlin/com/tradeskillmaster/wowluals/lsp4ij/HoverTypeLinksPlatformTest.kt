package com.tradeskillmaster.wowluals.lsp4ij

import com.intellij.platform.backend.documentation.DocumentationLinkHandler
import com.intellij.platform.backend.documentation.DocumentationTarget
import com.intellij.platform.backend.presentation.TargetPresentation
import com.intellij.model.Pointer
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import com.redhat.devtools.lsp4ij.client.features.FileUriSupport
import org.eclipse.lsp4j.MarkupContent
import org.eclipse.lsp4j.MarkupKind

/**
 * Clickable hover types through the real LSP4IJ pieces: its markdown renderer
 * (which our hover feature wraps) and the documentation link handler extension
 * point.
 */
class HoverTypeLinksPlatformTest : BasePlatformTestCase() {
    fun testHoverFeatureMovesTheGoToLinksOntoTheSignature() {
        val file = myFixture.configureByText("user.lua", "local rec = {}\n")
        val feature = WowLuaLanguageServerFactory().createClientFeatures().hoverFeature
        val markdown = "```wowlua-hover\n(local) rec: Record {\n  Range: Range\n}\n```\n---\nA record.\n\n---\n\n" +
            "Go to: [`Record`](file:///a/user.lua#L1) · [`Range`](file:///a/defs.lua#L3)"
        val html = feature.getContent(MarkupContent(MarkupKind.MARKDOWN, markdown), file)!!

        assertFalse("the Go to line is gone: $html", html.contains("Go to"))
        assertTrue("the doc stays: $html", html.contains("A record."))
        val contextUri = FileUriSupport.toString(file.virtualFile, feature.clientFeatures)!!
        val hash = project.locationHash
        val rangeHref = HoverTypeLinks.href(HoverTypeLinks.TypeLink("Range", "file:///a/defs.lua#L3"), hash, contextUri)
        val recordHref = HoverTypeLinks.href(HoverTypeLinks.TypeLink("Record", "file:///a/user.lua#L1"), hash, contextUri)
        // Each type is linked once: the field label `Range:` is not.
        assertEquals(html, 1, html.split("href=\"" + rangeHref.replace("&", "&amp;") + "\"").size - 1)
        assertEquals(html, 1, html.split("href=\"" + recordHref.replace("&", "&amp;") + "\"").size - 1)
    }

    fun testHoverFeatureLeavesAHoverWithoutLinksAlone() {
        val file = myFixture.configureByText("user.lua", "local n = 1\n")
        val feature = WowLuaLanguageServerFactory().createClientFeatures().hoverFeature
        val html = feature.getContent(MarkupContent(MarkupKind.MARKDOWN, "```wowlua-hover\n(local) n: number\n```"), file)!!
        assertFalse(html, html.contains("<a "))
    }

    fun testLinkHandlerOpensTheTypesHover() {
        val file = myFixture.configureByText("user.lua", "local rec = {}\n")
        val handler = DocumentationLinkHandler.EP_NAME.extensionList.single { it is WowLuaTypeLinkHandler }
        val contextUri = FileUriSupport.toString(file.virtualFile, FileUriSupport.DEFAULT)!!
        val link = HoverTypeLinks.TypeLink("Range", "file:///a/defs.lua#L3")
        val href = HoverTypeLinks.href(link, project.locationHash, contextUri)
        assertNotNull("our links resolve", handler.resolveLink(AnyTarget, href))
        assertNull("other links are left to LSP4IJ", handler.resolveLink(AnyTarget, "file:///a/defs.lua#L3"))
        assertNull("a closed project's links resolve to nothing", handler.resolveLink(AnyTarget, HoverTypeLinks.href(link, "gone", contextUri)))
    }

    private object AnyTarget : DocumentationTarget {
        override fun createPointer(): Pointer<out DocumentationTarget> = Pointer.hardPointer(this)
        override fun computePresentation(): TargetPresentation = TargetPresentation.builder("any").presentation()
    }
}
