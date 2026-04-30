package com.tradeskillmaster.wowluals

import com.intellij.lexer.Lexer
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.fileTypes.SyntaxHighlighter
import com.intellij.openapi.fileTypes.SyntaxHighlighterBase
import com.intellij.openapi.fileTypes.SyntaxHighlighterFactory
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.tree.IElementType

class WowLuaSyntaxHighlighter : SyntaxHighlighterBase() {
    override fun getHighlightingLexer(): Lexer = WowLuaLexer()

    override fun getTokenHighlights(tokenType: IElementType): Array<TextAttributesKey> = when (tokenType) {
        WowLuaTokenTypes.KEYWORD -> KEYWORD_KEYS
        WowLuaTokenTypes.CONSTANT -> CONSTANT_KEYS
        WowLuaTokenTypes.SELF -> SELF_KEYS
        WowLuaTokenTypes.STRING, WowLuaTokenTypes.LONG_STRING -> STRING_KEYS
        WowLuaTokenTypes.NUMBER -> NUMBER_KEYS
        WowLuaTokenTypes.COMMENT -> COMMENT_KEYS
        WowLuaTokenTypes.BLOCK_COMMENT -> BLOCK_COMMENT_KEYS
        WowLuaTokenTypes.OPERATOR -> OPERATOR_KEYS
        WowLuaTokenTypes.LPAREN, WowLuaTokenTypes.RPAREN -> PAREN_KEYS
        WowLuaTokenTypes.LBRACE, WowLuaTokenTypes.RBRACE -> BRACE_KEYS
        WowLuaTokenTypes.LBRACKET, WowLuaTokenTypes.RBRACKET -> BRACKET_KEYS
        WowLuaTokenTypes.COMMA -> COMMA_KEYS
        WowLuaTokenTypes.SEMICOLON -> SEMICOLON_KEYS
        WowLuaTokenTypes.DOT -> DOT_KEYS
        WowLuaTokenTypes.BAD_CHARACTER -> BAD_CHAR_KEYS
        else -> EMPTY_KEYS
    }

    companion object {
        private val KEYWORD = TextAttributesKey.createTextAttributesKey(
            "WOWLUA_KEYWORD", DefaultLanguageHighlighterColors.KEYWORD
        )
        private val CONSTANT = TextAttributesKey.createTextAttributesKey(
            "WOWLUA_CONSTANT", DefaultLanguageHighlighterColors.KEYWORD
        )
        private val SELF = TextAttributesKey.createTextAttributesKey(
            "WOWLUA_SELF", DefaultLanguageHighlighterColors.KEYWORD
        )
        private val STRING = TextAttributesKey.createTextAttributesKey(
            "WOWLUA_STRING", DefaultLanguageHighlighterColors.STRING
        )
        private val NUMBER = TextAttributesKey.createTextAttributesKey(
            "WOWLUA_NUMBER", DefaultLanguageHighlighterColors.NUMBER
        )
        private val COMMENT = TextAttributesKey.createTextAttributesKey(
            "WOWLUA_COMMENT", DefaultLanguageHighlighterColors.LINE_COMMENT
        )
        private val BLOCK_COMMENT = TextAttributesKey.createTextAttributesKey(
            "WOWLUA_BLOCK_COMMENT", DefaultLanguageHighlighterColors.BLOCK_COMMENT
        )
        private val OPERATOR = TextAttributesKey.createTextAttributesKey(
            "WOWLUA_OPERATOR", DefaultLanguageHighlighterColors.OPERATION_SIGN
        )
        private val PAREN = TextAttributesKey.createTextAttributesKey(
            "WOWLUA_PAREN", DefaultLanguageHighlighterColors.PARENTHESES
        )
        private val BRACE = TextAttributesKey.createTextAttributesKey(
            "WOWLUA_BRACE", DefaultLanguageHighlighterColors.BRACES
        )
        private val BRACKET = TextAttributesKey.createTextAttributesKey(
            "WOWLUA_BRACKET", DefaultLanguageHighlighterColors.BRACKETS
        )
        private val COMMA_ATTR = TextAttributesKey.createTextAttributesKey(
            "WOWLUA_COMMA", DefaultLanguageHighlighterColors.COMMA
        )
        private val SEMICOLON_ATTR = TextAttributesKey.createTextAttributesKey(
            "WOWLUA_SEMICOLON", DefaultLanguageHighlighterColors.SEMICOLON
        )
        private val DOT_ATTR = TextAttributesKey.createTextAttributesKey(
            "WOWLUA_DOT", DefaultLanguageHighlighterColors.DOT
        )
        private val BAD_CHAR = TextAttributesKey.createTextAttributesKey(
            "WOWLUA_BAD_CHARACTER", DefaultLanguageHighlighterColors.INVALID_STRING_ESCAPE
        )

        private val KEYWORD_KEYS = arrayOf(KEYWORD)
        private val CONSTANT_KEYS = arrayOf(CONSTANT)
        private val SELF_KEYS = arrayOf(SELF)
        private val STRING_KEYS = arrayOf(STRING)
        private val NUMBER_KEYS = arrayOf(NUMBER)
        private val COMMENT_KEYS = arrayOf(COMMENT)
        private val BLOCK_COMMENT_KEYS = arrayOf(BLOCK_COMMENT)
        private val OPERATOR_KEYS = arrayOf(OPERATOR)
        private val PAREN_KEYS = arrayOf(PAREN)
        private val BRACE_KEYS = arrayOf(BRACE)
        private val BRACKET_KEYS = arrayOf(BRACKET)
        private val COMMA_KEYS = arrayOf(COMMA_ATTR)
        private val SEMICOLON_KEYS = arrayOf(SEMICOLON_ATTR)
        private val DOT_KEYS = arrayOf(DOT_ATTR)
        private val BAD_CHAR_KEYS = arrayOf(BAD_CHAR)
        private val EMPTY_KEYS = emptyArray<TextAttributesKey>()
    }
}

class WowLuaSyntaxHighlighterFactory : SyntaxHighlighterFactory() {
    override fun getSyntaxHighlighter(project: Project?, virtualFile: VirtualFile?): SyntaxHighlighter =
        WowLuaSyntaxHighlighter()
}
