package com.tradeskillmaster.wowluals

import com.intellij.openapi.fileChooser.FileChooserDescriptorFactory
import com.intellij.openapi.options.Configurable
import com.intellij.openapi.ui.TextFieldWithBrowseButton
import com.intellij.util.ui.FormBuilder
import javax.swing.JComponent
import javax.swing.JPanel

class WowLuaSettingsConfigurable : Configurable {
    private var panel: JPanel? = null
    private var pathField: TextFieldWithBrowseButton? = null

    override fun getDisplayName(): String = "WoW Lua LS"

    override fun createComponent(): JComponent {
        val field = TextFieldWithBrowseButton()
        field.addBrowseFolderListener(
            "Select wowlua_ls Binary",
            "Path to the wowlua_ls language server binary. Leave empty to search PATH.",
            null,
            FileChooserDescriptorFactory.createSingleFileDescriptor()
        )
        pathField = field

        val form = FormBuilder.createFormBuilder()
            .addLabeledComponent("Server path:", field)
            .addComponentFillVertically(JPanel(), 0)
            .panel
        panel = form
        reset()
        return form
    }

    override fun isModified(): Boolean =
        pathField?.text != WowLuaSettings.getInstance().serverPath

    override fun apply() {
        WowLuaSettings.getInstance().serverPath = pathField?.text.orEmpty()
    }

    override fun reset() {
        pathField?.text = WowLuaSettings.getInstance().serverPath
    }

    override fun disposeUIResources() {
        panel = null
        pathField = null
    }
}
