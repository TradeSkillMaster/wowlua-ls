package com.tradeskillmaster.wowluals

import com.intellij.openapi.fileTypes.LanguageFileType
import javax.swing.Icon

class WowLuaFileType private constructor() : LanguageFileType(WowLuaLanguage.INSTANCE) {
    companion object {
        @JvmStatic
        val INSTANCE = WowLuaFileType()
    }

    override fun getName(): String = "Lua"
    override fun getDescription(): String = "Lua source file"
    override fun getDefaultExtension(): String = "lua"
    override fun getIcon(): Icon = WowLuaIcons.LUA_FILE
}
