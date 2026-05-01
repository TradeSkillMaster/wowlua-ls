package com.tradeskillmaster.wowluals

import com.intellij.ide.plugins.PluginManagerCore
import com.intellij.openapi.extensions.PluginId
import org.jetbrains.plugins.textmate.api.TextMateBundleProvider
import java.nio.file.Files

class WowLuaTextMateBundleProvider : TextMateBundleProvider {
    override fun getBundles(): List<TextMateBundleProvider.PluginBundle> {
        val pluginPath = PluginManagerCore.getPlugin(
            PluginId.getId("com.tradeskillmaster.wowlua-ls")
        )?.pluginPath ?: return emptyList()
        val bundlePath = pluginPath.resolve("textmate").resolve("lua")
        if (!Files.isDirectory(bundlePath)) return emptyList()
        return listOf(TextMateBundleProvider.PluginBundle("Lua", bundlePath))
    }
}
