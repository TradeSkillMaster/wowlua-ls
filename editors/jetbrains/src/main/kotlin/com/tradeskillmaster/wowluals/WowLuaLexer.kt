package com.tradeskillmaster.wowluals

import com.intellij.lexer.LexerBase
import com.intellij.psi.TokenType
import com.intellij.psi.tree.IElementType

class WowLuaLexer : LexerBase() {
    private var buffer: CharSequence = ""
    private var bufferEnd: Int = 0
    private var tokenStart: Int = 0
    private var tokenEnd: Int = 0
    private var tokenType: IElementType? = null

    override fun start(buffer: CharSequence, startOffset: Int, endOffset: Int, initialState: Int) {
        this.buffer = buffer
        this.bufferEnd = endOffset
        this.tokenStart = startOffset
        this.tokenEnd = startOffset
        this.tokenType = null
        advance()
    }

    override fun getState(): Int = 0
    override fun getTokenType(): IElementType? = tokenType
    override fun getTokenStart(): Int = tokenStart
    override fun getTokenEnd(): Int = tokenEnd
    override fun getBufferSequence(): CharSequence = buffer
    override fun getBufferEnd(): Int = bufferEnd

    override fun advance() {
        tokenStart = tokenEnd
        if (tokenStart >= bufferEnd) {
            tokenType = null
            return
        }

        val c = charAt(tokenStart)

        when {
            c.isWhitespace() -> lexWhitespace()
            c == '-' && charAt(tokenStart + 1) == '-' -> lexComment()
            c == '\'' || c == '"' -> lexShortString(c)
            c == '[' && isLongBracketStart(tokenStart) -> lexLongString()
            c.isDigit() -> lexNumber()
            c == '.' && charAt(tokenStart + 1).isDigit() -> lexNumber()
            c == '_' || c.isLetter() -> lexIdentifier()
            else -> lexOperatorOrPunctuation()
        }
    }

    private fun charAt(offset: Int): Char =
        if (offset in 0 until bufferEnd) buffer[offset] else '\u0000'

    private fun isLongBracketStart(pos: Int): Boolean {
        if (charAt(pos) != '[') return false
        var i = pos + 1
        while (i < bufferEnd && charAt(i) == '=') i++
        return charAt(i) == '['
    }

    private fun countEquals(pos: Int): Int {
        var count = 0
        var i = pos
        while (i < bufferEnd && charAt(i) == '=') {
            count++
            i++
        }
        return count
    }

    private fun findLongBracketEnd(start: Int, eqCount: Int): Int {
        var pos = start
        while (pos < bufferEnd) {
            if (charAt(pos) == ']') {
                var eq = 0
                var j = pos + 1
                while (j < bufferEnd && charAt(j) == '=' && eq < eqCount) {
                    eq++
                    j++
                }
                if (eq == eqCount && charAt(j) == ']') {
                    return j + 1
                }
            }
            pos++
        }
        return bufferEnd
    }

    private fun lexWhitespace() {
        var pos = tokenStart
        while (pos < bufferEnd && charAt(pos).isWhitespace()) pos++
        tokenEnd = pos
        tokenType = TokenType.WHITE_SPACE
    }

    private fun lexComment() {
        var pos = tokenStart + 2

        if (charAt(pos) == '[') {
            val eqCount = countEquals(pos + 1)
            if (charAt(pos + 1 + eqCount) == '[') {
                tokenEnd = findLongBracketEnd(pos + 2 + eqCount, eqCount)
                tokenType = WowLuaTokenTypes.BLOCK_COMMENT
                return
            }
        }

        while (pos < bufferEnd && charAt(pos) != '\n') pos++
        tokenEnd = pos
        tokenType = WowLuaTokenTypes.COMMENT
    }

    private fun lexShortString(quote: Char) {
        var pos = tokenStart + 1
        while (pos < bufferEnd) {
            val c = charAt(pos)
            when {
                c == '\\' -> pos += 2
                c == quote -> { pos++; break }
                c == '\n' -> break
                else -> pos++
            }
        }
        tokenEnd = pos
        tokenType = WowLuaTokenTypes.STRING
    }

    private fun lexLongString() {
        val eqCount = countEquals(tokenStart + 1)
        val contentStart = tokenStart + 2 + eqCount
        tokenEnd = findLongBracketEnd(contentStart, eqCount)
        tokenType = WowLuaTokenTypes.LONG_STRING
    }

    private fun lexNumber() {
        var pos = tokenStart

        if (charAt(pos) == '0' && (charAt(pos + 1) == 'x' || charAt(pos + 1) == 'X')) {
            pos += 2
            while (pos < bufferEnd && isHexDigit(charAt(pos))) pos++
            if (charAt(pos) == '.') {
                pos++
                while (pos < bufferEnd && isHexDigit(charAt(pos))) pos++
            }
            if (charAt(pos) == 'p' || charAt(pos) == 'P') {
                pos++
                if (charAt(pos) == '+' || charAt(pos) == '-') pos++
                while (pos < bufferEnd && charAt(pos).isDigit()) pos++
            }
        } else {
            while (pos < bufferEnd && charAt(pos).isDigit()) pos++
            if (charAt(pos) == '.' && charAt(pos + 1) != '.') {
                pos++
                while (pos < bufferEnd && charAt(pos).isDigit()) pos++
            }
            if (charAt(pos) == 'e' || charAt(pos) == 'E') {
                pos++
                if (charAt(pos) == '+' || charAt(pos) == '-') pos++
                while (pos < bufferEnd && charAt(pos).isDigit()) pos++
            }
        }

        tokenEnd = pos
        tokenType = WowLuaTokenTypes.NUMBER
    }

    private fun lexIdentifier() {
        var pos = tokenStart
        while (pos < bufferEnd && (charAt(pos).isLetterOrDigit() || charAt(pos) == '_')) pos++
        tokenEnd = pos

        val word = buffer.subSequence(tokenStart, tokenEnd)
        tokenType = when {
            word.isKeyword() -> WowLuaTokenTypes.KEYWORD
            word.isConstant() -> WowLuaTokenTypes.CONSTANT
            word.contentEquals("self") -> WowLuaTokenTypes.SELF
            else -> WowLuaTokenTypes.IDENTIFIER
        }
    }

    private fun lexOperatorOrPunctuation() {
        val c = charAt(tokenStart)
        var pos = tokenStart + 1

        tokenType = when (c) {
            '(' -> WowLuaTokenTypes.LPAREN
            ')' -> WowLuaTokenTypes.RPAREN
            '{' -> WowLuaTokenTypes.LBRACE
            '}' -> WowLuaTokenTypes.RBRACE
            '[' -> WowLuaTokenTypes.LBRACKET
            ']' -> WowLuaTokenTypes.RBRACKET
            ';' -> WowLuaTokenTypes.SEMICOLON
            ',' -> WowLuaTokenTypes.COMMA
            '.' -> {
                if (charAt(pos) == '.') {
                    pos++
                    if (charAt(pos) == '.') pos++
                    WowLuaTokenTypes.OPERATOR
                } else {
                    WowLuaTokenTypes.DOT
                }
            }
            '+', '*', '/', '%', '^', '#' -> WowLuaTokenTypes.OPERATOR
            '-' -> WowLuaTokenTypes.OPERATOR
            '~' -> {
                if (charAt(pos) == '=') pos++
                WowLuaTokenTypes.OPERATOR
            }
            '=' -> {
                if (charAt(pos) == '=') pos++
                WowLuaTokenTypes.OPERATOR
            }
            '<' -> {
                if (charAt(pos) == '=' || charAt(pos) == '<') pos++
                WowLuaTokenTypes.OPERATOR
            }
            '>' -> {
                if (charAt(pos) == '=' || charAt(pos) == '>') pos++
                WowLuaTokenTypes.OPERATOR
            }
            ':' -> {
                if (charAt(pos) == ':') pos++
                WowLuaTokenTypes.OPERATOR
            }
            else -> WowLuaTokenTypes.BAD_CHARACTER
        }

        tokenEnd = pos
    }

    private fun isHexDigit(c: Char): Boolean =
        c.isDigit() || c in 'a'..'f' || c in 'A'..'F'

    companion object {
        private val KEYWORDS = setOf(
            "and", "break", "do", "else", "elseif", "end",
            "for", "function", "goto", "if", "in", "local",
            "not", "or", "repeat", "return", "then", "until", "while"
        )

        private val CONSTANTS = setOf("true", "false", "nil")

        private fun CharSequence.isKeyword(): Boolean = this.toString() in KEYWORDS
        private fun CharSequence.isConstant(): Boolean = this.toString() in CONSTANTS
    }
}
