package com.tradeskillmaster.wowluals.lsp4ij

import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiFile
import com.redhat.devtools.lsp4ij.LanguageServerEnablementSupport
import com.redhat.devtools.lsp4ij.LanguageServerFactory
import com.redhat.devtools.lsp4ij.client.features.LSPClientFeatures
import com.redhat.devtools.lsp4ij.client.features.LSPSemanticTokensFeature
import com.redhat.devtools.lsp4ij.server.ProcessStreamConnectionProvider
import com.redhat.devtools.lsp4ij.server.StreamConnectionProvider
import com.tradeskillmaster.wowluals.WowLuaBackend
import com.tradeskillmaster.wowluals.WowLuaServerPath
import com.tradeskillmaster.wowluals.WowLuaSettings

/**
 * LSP4IJ backend. Registered via the optional lsp4ij.xml config, so this
 * class only loads when the LSP4IJ plugin is installed; whether it *runs* is
 * decided by [WowLuaBackend] (opt-in setting, or automatic on IDEs without
 * the built-in LSP client).
 */
class WowLuaLanguageServerFactory : LanguageServerFactory, LanguageServerEnablementSupport {

    override fun createConnectionProvider(project: Project): StreamConnectionProvider =
        object : ProcessStreamConnectionProvider(listOf(WowLuaServerPath.resolve()), project.basePath ?: ".") {}

    override fun createClientFeatures(): LSPClientFeatures =
        LSPClientFeatures().setSemanticTokensFeature(object : LSPSemanticTokensFeature() {
            // Same mapping as the native descriptor: the server emits the
            // non-standard `builtinConstant` token type for boolean / nil
            // literals inside `expression<>` strings; map it to the IDE's
            // constant color.
            override fun getTextAttributesKey(
                tokenType: String,
                tokenModifiers: List<String>,
                file: PsiFile,
            ): TextAttributesKey? =
                if (tokenType == "builtinConstant") DefaultLanguageHighlighterColors.CONSTANT
                else super.getTextAttributesKey(tokenType, tokenModifiers, file)
        })

    // LSP4IJ's "Language Servers" UI drives these. Keep server-level enablement
    // separate from backend selection (useLsp4ij): unchecking the server here
    // must turn LSP4IJ off without silently re-activating the built-in backend,
    // and must be honored even on Community IDEs where useLsp4ij() is forced true.
    override fun isEnabled(project: Project): Boolean =
        WowLuaBackend.useLsp4ij() && !WowLuaSettings.getInstance().lsp4ijServerDisabled

    override fun setEnabled(enabled: Boolean, project: Project) {
        WowLuaSettings.getInstance().lsp4ijServerDisabled = !enabled
    }
}
