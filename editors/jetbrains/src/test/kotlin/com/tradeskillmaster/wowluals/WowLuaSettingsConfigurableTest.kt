package com.tradeskillmaster.wowluals

import com.intellij.openapi.ui.TextFieldWithBrowseButton
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import com.intellij.util.ui.UIUtil

/**
 * Setting a server path must turn the server back on. LSP4IJ turns a server off
 * by itself after 20 failed starts, saved through the factory's `setEnabled`
 * into [WowLuaSettings.lsp4ijServerDisabled], and a missing or wrong server path
 * is what fails them: the user who then sets the path the error asked for would
 * otherwise get no server at all, even after an IDE restart.
 */
class WowLuaSettingsConfigurableTest : BasePlatformTestCase() {
    private var savedPath = ""
    private var savedDisabled = false

    override fun setUp() {
        super.setUp()
        savedPath = WowLuaServerPathSettings.getInstance().serverPath
        savedDisabled = WowLuaSettings.getInstance().lsp4ijServerDisabled
    }

    override fun tearDown() {
        try {
            WowLuaServerPathSettings.getInstance().serverPath = savedPath
            WowLuaSettings.getInstance().lsp4ijServerDisabled = savedDisabled
        } finally {
            super.tearDown()
        }
    }

    private fun applyServerPath(path: String) {
        val configurable = WowLuaSettingsConfigurable()
        try {
            val panel = configurable.createComponent()!!
            configurable.reset()
            UIUtil.findComponentOfType(panel, TextFieldWithBrowseButton::class.java)!!.text = path
            configurable.apply()
        } finally {
            configurable.disposeUIResources()
        }
    }

    fun testSettingAServerPathTurnsTheServerBackOn() {
        WowLuaServerPathSettings.getInstance().serverPath = ""
        WowLuaSettings.getInstance().lsp4ijServerDisabled = true

        applyServerPath("/opt/wowlua/wowlua_ls")

        assertEquals("/opt/wowlua/wowlua_ls", WowLuaServerPathSettings.getInstance().serverPath)
        assertFalse(WowLuaSettings.getInstance().lsp4ijServerDisabled)
    }

    fun testApplyingAnUnchangedPathLeavesATurnedOffServerOff() {
        WowLuaServerPathSettings.getInstance().serverPath = "/opt/wowlua/wowlua_ls"
        WowLuaSettings.getInstance().lsp4ijServerDisabled = true

        applyServerPath("/opt/wowlua/wowlua_ls")

        assertTrue(WowLuaSettings.getInstance().lsp4ijServerDisabled)
    }
}
