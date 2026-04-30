package com.tradeskillmaster.wowluals

import com.intellij.psi.tree.IElementType
import com.intellij.psi.tree.TokenSet

object WowLuaTokenTypes {
    @JvmField val KEYWORD = IElementType("KEYWORD", WowLuaLanguage.INSTANCE)
    @JvmField val CONSTANT = IElementType("CONSTANT", WowLuaLanguage.INSTANCE)
    @JvmField val SELF = IElementType("SELF", WowLuaLanguage.INSTANCE)
    @JvmField val IDENTIFIER = IElementType("IDENTIFIER", WowLuaLanguage.INSTANCE)
    @JvmField val NUMBER = IElementType("NUMBER", WowLuaLanguage.INSTANCE)
    @JvmField val STRING = IElementType("STRING", WowLuaLanguage.INSTANCE)
    @JvmField val LONG_STRING = IElementType("LONG_STRING", WowLuaLanguage.INSTANCE)
    @JvmField val COMMENT = IElementType("COMMENT", WowLuaLanguage.INSTANCE)
    @JvmField val BLOCK_COMMENT = IElementType("BLOCK_COMMENT", WowLuaLanguage.INSTANCE)
    @JvmField val OPERATOR = IElementType("OPERATOR", WowLuaLanguage.INSTANCE)
    @JvmField val LPAREN = IElementType("LPAREN", WowLuaLanguage.INSTANCE)
    @JvmField val RPAREN = IElementType("RPAREN", WowLuaLanguage.INSTANCE)
    @JvmField val LBRACE = IElementType("LBRACE", WowLuaLanguage.INSTANCE)
    @JvmField val RBRACE = IElementType("RBRACE", WowLuaLanguage.INSTANCE)
    @JvmField val LBRACKET = IElementType("LBRACKET", WowLuaLanguage.INSTANCE)
    @JvmField val RBRACKET = IElementType("RBRACKET", WowLuaLanguage.INSTANCE)
    @JvmField val COMMA = IElementType("COMMA", WowLuaLanguage.INSTANCE)
    @JvmField val SEMICOLON = IElementType("SEMICOLON", WowLuaLanguage.INSTANCE)
    @JvmField val DOT = IElementType("DOT", WowLuaLanguage.INSTANCE)
    @JvmField val BAD_CHARACTER = IElementType("BAD_CHARACTER", WowLuaLanguage.INSTANCE)

    @JvmField
    val COMMENTS = TokenSet.create(COMMENT, BLOCK_COMMENT)

    @JvmField
    val STRINGS = TokenSet.create(STRING, LONG_STRING)
}
