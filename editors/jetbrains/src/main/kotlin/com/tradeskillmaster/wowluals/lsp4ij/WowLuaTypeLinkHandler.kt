package com.tradeskillmaster.wowluals.lsp4ij

import com.intellij.openapi.project.ProjectManager
import com.intellij.platform.backend.documentation.DocumentationLinkHandler
import com.intellij.platform.backend.documentation.DocumentationTarget
import com.intellij.platform.backend.documentation.LinkResolveResult

/**
 * Resolves a click on a type [WowLuaHoverFeature] linked in a hover to that
 * type's own hover, shown in the same popup (with IntelliJ's back/forward
 * navigation, and Jump to Source to its declaration).
 */
class WowLuaTypeLinkHandler : DocumentationLinkHandler {
    override fun resolveLink(target: DocumentationTarget, url: String): LinkResolveResult? {
        val link = HoverTypeLinks.parseHref(url) ?: return null
        val project = ProjectManager.getInstance().openProjects
            .firstOrNull { !it.isDisposed && it.locationHash == link.projectHash } ?: return null
        return LinkResolveResult.resolvedTarget(WowLuaTypeDocumentationTarget(project, link))
    }
}
