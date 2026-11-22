package com.tradeskillmaster.wowluals.lsp4ij

import com.intellij.model.Pointer
import com.intellij.openapi.application.readAction
import com.intellij.openapi.fileEditor.OpenFileDescriptor
import com.intellij.openapi.project.Project
import com.intellij.platform.backend.documentation.DocumentationResult
import com.intellij.platform.backend.documentation.DocumentationTarget
import com.intellij.platform.backend.presentation.TargetPresentation
import com.intellij.pom.Navigatable
import com.intellij.psi.PsiManager
import com.redhat.devtools.lsp4ij.LanguageServerManager
import com.redhat.devtools.lsp4ij.client.features.FileUriSupport
import kotlinx.coroutines.future.await
import org.eclipse.lsp4j.TextDocumentIdentifier

/**
 * A type's hover, shown in the popup when the type is clicked in another hover
 * ([WowLuaTypeLinkHandler]). The server reads the name in the file the first
 * hover came from (`wowlua/typeHover`), so a class that file declares resolves
 * as it does there. Jump to Source (F4) goes to the type's declaration.
 */
internal class WowLuaTypeDocumentationTarget(
    private val project: Project,
    private val link: HoverTypeLinks.LinkTarget,
) : DocumentationTarget {
    override fun createPointer(): Pointer<out DocumentationTarget> = Pointer.hardPointer(this)

    override fun computePresentation(): TargetPresentation = TargetPresentation.builder(link.name).presentation()

    override val navigatable: Navigatable?
        get() {
            val (uri, line) = HoverTypeLinks.declarationLine(link.declarationUrl) ?: return null
            val file = FileUriSupport.findFileByUri(uri, FileUriSupport.DEFAULT) ?: return null
            return OpenFileDescriptor(project, file, line, 0)
        }

    override fun computeDocumentation(): DocumentationResult = DocumentationResult.asyncDocumentation {
        val item = LanguageServerManager.getInstance(project).getLanguageServer(SERVER_ID).await()
            ?: return@asyncDocumentation null
        val server = item.server as? WowLuaLanguageServerApi ?: return@asyncDocumentation null
        val params = TypeHoverParams(TextDocumentIdentifier(link.contextUri), link.name)
        val content = server.typeHover(params).await()?.contents?.right ?: return@asyncDocumentation null
        // Rendered as LSP4IJ renders a hover, through our feature, so this hover's
        // own types link on.
        val html = readAction {
            FileUriSupport.findFileByUri(link.contextUri, item.clientFeatures)
                ?.let { PsiManager.getInstance(project).findFile(it) }
                ?.let { item.clientFeatures.hoverFeature.getContent(content, it) }
        }
        html?.let { DocumentationResult.documentation(it) }
    }

    private companion object {
        const val SERVER_ID = "wowlua-ls"
    }
}
