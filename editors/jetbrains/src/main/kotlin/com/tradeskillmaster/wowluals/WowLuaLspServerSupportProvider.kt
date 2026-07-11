package com.tradeskillmaster.wowluals

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspServerSupportProvider
import com.intellij.platform.lsp.api.ProjectWideLspServerDescriptor
import com.intellij.platform.lsp.api.customization.LspCustomization
import com.intellij.platform.lsp.api.customization.LspSemanticTokensSupport
import java.io.File

class WowLuaLspServerSupportProvider : LspServerSupportProvider {
    override fun fileOpened(project: Project, file: VirtualFile, serverStarter: LspServerSupportProvider.LspServerStarter) {
        // Stand down when the user has switched to the LSP4IJ backend, so the
        // two clients never serve the same file simultaneously.
        if (WowLuaBackend.useLsp4ij()) return
        if (file.extension == "lua" || file.extension == "toc") {
            serverStarter.ensureServerStarted(WowLuaLspServerDescriptor(project))
        }
    }
}

private class WowLuaLspServerDescriptor(project: Project) : ProjectWideLspServerDescriptor(project, "WoW Lua LS") {
    // The server emits the non-standard `builtinConstant` semantic-token type for
    // boolean / nil literals inside `expression<>` strings (so VS Code can map it to
    // `constant.language`). IntelliJ only maps the standard LSP token types, so map it
    // here to the IDE's constant color — otherwise `true`/`false`/`nil` would lose their
    // highlighting (fall back to the surrounding string scope).
    override val lspCustomization: LspCustomization = object : LspCustomization() {
        override val semanticTokensCustomizer = object : LspSemanticTokensSupport() {
            override fun getTextAttributesKey(tokenType: String, tokenModifiers: List<String>): TextAttributesKey? =
                if (tokenType == "builtinConstant") DefaultLanguageHighlighterColors.CONSTANT
                else super.getTextAttributesKey(tokenType, tokenModifiers)
        }
    }

    override fun isSupportedFile(file: VirtualFile) = file.extension == "lua" || file.extension == "toc"

    override fun createCommandLine(): GeneralCommandLine {
        val commandLine = GeneralCommandLine(WowLuaServerPath.resolve())
        commandLine.workDirectory = File(project.basePath ?: ".")
        return commandLine
    }
}
