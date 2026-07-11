package com.tradeskillmaster.wowluals

import com.intellij.openapi.fileEditor.FileEditor
import com.intellij.openapi.project.Project
import com.intellij.openapi.updateSettings.impl.pluginsAdvertisement.PluginSuggestion
import com.intellij.openapi.updateSettings.impl.pluginsAdvertisement.PluginSuggestionProvider
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.ui.EditorNotificationPanel

/**
 * Suppresses the "Plugins supporting *.lua files found" advertiser banner.
 *
 * The platform shows it because no installed plugin registers a `.lua`/`.toc`
 * FileType (deliberate on our side — a FileType would disable the TextMate
 * coloration). A provider's suggestion takes priority over the default
 * marketplace banner, and the editor-notification contract allows returning
 * no panel — so claiming the file and rendering nothing hides the banner.
 *
 * PluginSuggestionProvider/PluginSuggestion are @ApiStatus.Internal, but they
 * are the only suppression surface the platform exposes. If the API shape
 * changes in a future IDE, this class fails to load and the banner reappears —
 * a cosmetic regression, not a crash.
 */
class WowLuaPluginSuggestionSuppressor : PluginSuggestionProvider {
    override fun getSuggestion(project: Project, file: VirtualFile): PluginSuggestion? =
        if (file.extension == "lua" || file.extension == "toc") SuppressedSuggestion else null

    private object SuppressedSuggestion : PluginSuggestion {
        override val pluginIds: List<String> = emptyList()
        override fun apply(editor: FileEditor): EditorNotificationPanel? = null
    }
}
