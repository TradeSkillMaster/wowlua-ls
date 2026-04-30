package com.tradeskillmaster.wowluals

import com.intellij.lang.Language

class WowLuaLanguage private constructor() : Language("WowLua", "text/x-lua") {
    companion object {
        @JvmStatic
        val INSTANCE = WowLuaLanguage()
    }

    override fun getDisplayName(): String = "Lua"
}
