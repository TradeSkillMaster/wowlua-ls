package com.tradeskillmaster.wowluals

import com.intellij.openapi.fileChooser.FileChooserDescriptorFactory
import com.intellij.openapi.options.BoundConfigurable
import com.intellij.openapi.project.ProjectManager
import com.intellij.openapi.ui.DialogPanel
import com.intellij.ui.dsl.builder.AlignX
import com.intellij.ui.dsl.builder.bindText
import com.intellij.ui.dsl.builder.panel
import com.redhat.devtools.lsp4ij.LanguageServerManager

/**
 * Settings | Languages & Frameworks | WoW Lua LS: the user's own server binary.
 * An application-level page, so no project can set what it runs.
 */
class WowLuaSettingsConfigurable : BoundConfigurable("WoW Lua LS") {
    private val settings = WowLuaServerPathSettings.getInstance()

    override fun createPanel(): DialogPanel = panel {
        row("Server path:") {
            textFieldWithBrowseButton(
                FileChooserDescriptorFactory.singleFile().withTitle("Select the wowlua_ls Binary"),
            )
                .bindText(settings::serverPath)
                .align(AlignX.FILL)
                .comment(
                    "Absolute path to a wowlua_ls binary to run instead of the bundled one: for a platform " +
                        "the plugin has no bundled server for, or to run one you built. Leave empty to use " +
                        "the bundled server.",
                )
        }
    }

    override fun apply() {
        val before = settings.serverPath
        super.apply()
        if (settings.serverPath != before) startWithNewPath()
    }

    /**
     * Puts the new path to use in every open project without an IDE restart, and
     * turns the server back on. LSP4IJ turns a server off by itself after 20
     * failed starts, saved through the factory's `setEnabled`, and a missing or
     * wrong server path is exactly what fails them: a user who sets the path the
     * error asked for would otherwise get no server, even after a restart.
     */
    private fun startWithNewPath() {
        WowLuaSettings.getInstance().lsp4ijServerDisabled = false
        for (project in ProjectManager.getInstance().openProjects) {
            if (project.isDisposed) continue
            // Restarts a running or failed server, and starts one for a project
            // with Lua files open that has none.
            LanguageServerManager.getInstance(project).start(
                SERVER_ID,
                LanguageServerManager.StartOptions().setWillEnable(true).setForceRestart(true),
            )
        }
    }

    private companion object {
        const val SERVER_ID = "wowlua-ls"
    }
}
