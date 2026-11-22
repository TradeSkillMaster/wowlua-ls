package com.tradeskillmaster.wowluals.lsp4ij

import com.intellij.psi.PsiFile
import com.redhat.devtools.lsp4ij.client.features.FileUriSupport
import com.redhat.devtools.lsp4ij.client.features.LSPHoverFeature
import org.eclipse.lsp4j.MarkupContent
import org.eclipse.lsp4j.MarkupKind

/**
 * Makes the types in a hover's signature clickable: the server's `Go to:` line
 * of type links comes off the bottom of the hover and goes onto the names in the
 * highlighted signature, each linking to that type's own hover (see
 * [HoverTypeLinks]). LSP4IJ renders [WowLuaTypeDocumentationTarget]'s content
 * through here too, so the types in a clicked type's hover link on in turn.
 */
class WowLuaHoverFeature : LSPHoverFeature() {
    override fun getContent(content: MarkupContent, file: PsiFile): String? {
        if (content.kind != MarkupKind.MARKDOWN) return super.getContent(content, file)
        val (markdown, links) = HoverTypeLinks.splitFooter(content.value)
        val virtualFile = file.virtualFile
        if (links.isEmpty() || virtualFile == null) return super.getContent(content, file)
        val html = super.getContent(MarkupContent(content.kind, markdown), file) ?: return null
        val contextUri = FileUriSupport.toString(virtualFile, clientFeatures) ?: return html
        val projectHash = file.project.locationHash
        return HoverTypeLinks.linkify(html, links.associate { it.name to HoverTypeLinks.href(it, projectHash, contextUri) })
    }
}
