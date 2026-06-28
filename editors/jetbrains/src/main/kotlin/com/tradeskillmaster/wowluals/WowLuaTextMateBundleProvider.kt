package com.tradeskillmaster.wowluals

import com.intellij.openapi.application.PluginPathManager
import org.jetbrains.plugins.textmate.api.TextMateBundleProvider
import java.nio.file.Files

class WowLuaTextMateBundleProvider : TextMateBundleProvider {
    override fun getBundles(): List<TextMateBundleProvider.PluginBundle> {
        // Resolve <pluginPath>/textmate/ from this plugin's own dist directory via the
        // public PluginPathManager API (no internal PluginManagerCore / hardcoded ID).
        val textmateDir = PluginPathManager.getPluginResource(javaClass, "textmate")?.toPath()
            ?: return emptyList()
        val bundles = mutableListOf<TextMateBundleProvider.PluginBundle>()
        val luaBundle = textmateDir.resolve("lua")
        if (Files.isDirectory(luaBundle)) {
            bundles.add(TextMateBundleProvider.PluginBundle("Lua", luaBundle))
        }
        val tocBundle = textmateDir.resolve("toc")
        if (Files.isDirectory(tocBundle)) {
            bundles.add(TextMateBundleProvider.PluginBundle("TOC", tocBundle))
        }
        return bundles
    }
}
