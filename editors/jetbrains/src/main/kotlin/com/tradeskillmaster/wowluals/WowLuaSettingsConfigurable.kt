package com.tradeskillmaster.wowluals

import com.intellij.openapi.options.Configurable
import com.intellij.ui.components.JBCheckBox
import com.intellij.ui.components.JBLabel
import com.intellij.util.ui.FormBuilder
import com.intellij.util.ui.UIUtil
import javax.swing.JComponent
import javax.swing.JPanel

class WowLuaSettingsConfigurable : Configurable {
    private var panel: JPanel? = null
    private var lsp4ijCheckBox: JBCheckBox? = null

    override fun getDisplayName(): String = "WoW Lua LS"

    override fun createComponent(): JComponent {
        val checkBox = JBCheckBox("Use the LSP4IJ plugin instead of the IDE's built-in LSP client")
        // The toggle only represents a real choice when both backends exist:
        // without LSP4IJ there is nothing to switch to, and without the
        // built-in LSP module (Community-based IDEs) LSP4IJ is used
        // automatically as the only backend.
        checkBox.isEnabled = WowLuaBackend.lsp4ijAvailable && WowLuaBackend.nativeLspAvailable
        lsp4ijCheckBox = checkBox

        val note = when {
            !WowLuaBackend.lsp4ijAvailable ->
                "Install the LSP4IJ plugin from the Marketplace to enable this option."
            !WowLuaBackend.nativeLspAvailable ->
                "This IDE has no built-in LSP client; the LSP4IJ backend is used automatically."
            else ->
                "Takes effect after the IDE restarts."
        }
        val noteLabel = JBLabel(note)
        noteLabel.componentStyle = UIUtil.ComponentStyle.SMALL
        noteLabel.foreground = UIUtil.getContextHelpForeground()

        val form = FormBuilder.createFormBuilder()
            .addComponent(checkBox)
            .addComponent(noteLabel)
            .addComponentFillVertically(JPanel(), 0)
            .panel
        panel = form
        reset()
        return form
    }

    override fun isModified(): Boolean =
        lsp4ijCheckBox?.isSelected != WowLuaSettings.getInstance().useLsp4ij

    override fun apply() {
        val settings = WowLuaSettings.getInstance()
        val useLsp4ij = lsp4ijCheckBox?.isSelected ?: false
        settings.useLsp4ij = useLsp4ij
        // Explicitly selecting the LSP4IJ backend here is a stronger signal than a
        // lingering server-level disable from LSP4IJ's own UI; clear it so the
        // server actually starts rather than staying silently off.
        if (useLsp4ij) settings.lsp4ijServerDisabled = false
    }

    override fun reset() {
        lsp4ijCheckBox?.isSelected = WowLuaSettings.getInstance().useLsp4ij
    }

    override fun disposeUIResources() {
        panel = null
        lsp4ijCheckBox = null
    }
}
