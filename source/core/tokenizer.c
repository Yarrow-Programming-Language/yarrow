#include <assert.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "tokenizer.h"

static void
tokenizer_error(const char *format, ...)
{
	va_list args;

	va_start(args, format);
	fprintf(stderr, "ERROR: ");
	vfprintf(stderr, format, args);
	fprintf(stderr, "\n");
	va_end(args);
}


const char *
tokenizer_kind_name(TokenKind kind)
{
	switch (kind)
	{
		case TOKEN_LEFT_PAREN: return "left_paren";
		case TOKEN_RIGHT_PAREN: return "right_paren";
		case TOKEN_LEFT_CURLY: return "left_curly";
		case TOKEN_RIGHT_CURLY: return "right_curly";
		case TOKEN_LEFT_SQUARE: return "left_square";
		case TOKEN_RIGHT_SQUARE: return "right_square";
		case TOKEN_PLUS: return "plus";
		case TOKEN_MINUS: return "minus";
		case TOKEN_ASTERISK: return "asterisk";
		case TOKEN_SLASH: return "slash";
		case TOKEN_SLASH_SLASH: return "slash_slash";
		case TOKEN_PERCENT: return "percent";
		case TOKEN_TILDE: return "tilde";
		case TOKEN_CARET: return "caret";
		case TOKEN_PIPE: return "pipe";
		case TOKEN_DOT: return "dot";
		case TOKEN_UNDERSCORE: return "underscore";
		case TOKEN_BANG: return "bang";
		case TOKEN_EQUAL: return "equal";
		case TOKEN_EQUAL_EQUAL: return "equal_equal";
		case TOKEN_NOT_EQUAL: return "not_equal";
		case TOKEN_GREATER: return "greater";
		case TOKEN_GREATER_EQUAL: return "greater_equal";
		case TOKEN_LESS: return "less";
		case TOKEN_LESS_EQUAL: return "less_equal";
		case TOKEN_AND: return "and";
		case TOKEN_OR: return "or";
		case TOKEN_NOT: return "not";
		case TOKEN_XOR: return "xor";
		case TOKEN_LEFT_SHIFT: return "left_shift";
		case TOKEN_RIGHT_SHIFT: return "right_shift";
		case TOKEN_AT: return "at";
		case TOKEN_IDENTIFIER: return "identifier";
		case TOKEN_TYPE: return "type";
		case TOKEN_STRING: return "string";
		case TOKEN_RAW_STRING: return "raw_string";
		case TOKEN_RUNE: return "rune";
		case TOKEN_INTEGER: return "integer";
		case TOKEN_BINARY_INTEGER: return "binary_integer";
		case TOKEN_OCTAL_INTEGER: return "octal_integer";
		case TOKEN_HEX_INTEGER: return "hex_integer";
		case TOKEN_FLOAT: return "float";
		case TOKEN_SCIENTIFIC_FLOAT: return "scientific_float";
		case TOKEN_TRUE: return "true";
		case TOKEN_FALSE: return "false";
		case TOKEN_IF: return "if";
		case TOKEN_ELSE: return "else";
		case TOKEN_FOR: return "for";
		case TOKEN_CONTINUE: return "continue";
		case TOKEN_BREAK: return "break";
		case TOKEN_MATCH: return "match";
		case TOKEN_CASE: return "case";
		case TOKEN_AS: return "as";
		case TOKEN_UNWRAP: return "unwrap";
		case TOKEN_TRY: return "try";
		case TOKEN_HANDLE: return "handle";
		case TOKEN_FUNCTION: return "function";
		case TOKEN_RETURN: return "return";
		case TOKEN_TYPEOF: return "typeof";
		case TOKEN_SIZEOF: return "sizeof";
		case TOKEN_CALL: return "call";
		case TOKEN_DO: return "do";
		case TOKEN_WITH: return "with";
		case TOKEN_END: return "end";
		case TOKEN_CONST: return "const";
		case TOKEN_STATIC: return "static";
		case TOKEN_MUTABLE: return "mutable";
		case TOKEN_SET: return "set";
		case TOKEN_UNSAFE: return "unsafe";
		case TOKEN_PUBLIC: return "public";
		case TOKEN_PRIVATE: return "private";
		case TOKEN_ERROR: return "error";
		case TOKEN_STRUCT: return "struct";
		case TOKEN_IMPLEMENT: return "implement";
		case TOKEN_ENUM: return "enum";
		case TOKEN_UNION: return "union";
		case TOKEN_REQUIRE: return "require";
		case TOKEN_DEFER: return "defer";
		case TOKEN_INC: return "inc";
		case TOKEN_DEC: return "dec";
		case TOKEN_POP: return "pop";
		case TOKEN_DROP: return "drop";
		case TOKEN_DUP: return "dup";
		case TOKEN_ROT: return "rot";
		case TOKEN_UNROT: return "unrot";
		case TOKEN_SWAP: return "swap";
		case TOKEN_BORROW: return "borrow";
		case TOKEN_MUTABLE_BORROW: return "mutable_borrow";
		case TOKEN_MOVE: return "move";
		case TOKEN_LOAD: return "load";
		case TOKEN_STORE: return "store";
		case TOKEN_VOID: return "void";
		case TOKEN_NULL: return "null";
		case TOKEN_EOF: return "eof";
	}

	return "unknown";
}

static int
tokenizer_grow_tokens(Token **tokens, size_t *capacity)
{
	size_t new_capacity = *capacity == 0 ? 64 : *capacity * 2;
	Token *grow = realloc(*tokens, new_capacity * sizeof(Token));
	if (grow == NULL)
	{
		return EXIT_FAILURE;
	}

	*tokens = grow;
	*capacity = new_capacity;
	return EXIT_SUCCESS;
}

static char
tokenizer_peek(const Tokenizer *tokenizer)
{
	return *tokenizer->current;
}

static char
tokenizer_peek_next(const Tokenizer *tokenizer)
{
	if (*tokenizer->current == '\0')
	{
		return '\0';
	}

	return tokenizer->current[1];
}

static char
tokenizer_advance(Tokenizer *tokenizer)
{
	char character = *tokenizer->current++;

	if (character == '\n')
	{
		tokenizer->line++;
		tokenizer->column = 1;
	}
	else
	{
		tokenizer->column++;
	}

	return character;
}

static int
tokenizer_skip_line(Tokenizer *tokenizer)
{
	while(tokenizer_peek(tokenizer) != '\n' && tokenizer_peek(tokenizer) != '\0')
	{
		tokenizer_advance(tokenizer);
	}

	return EXIT_SUCCESS;
}

static bool
tokenizer_match(Tokenizer *tokenizer, char character)
{
	if (*tokenizer->current == character)
	{
		tokenizer_advance(tokenizer);
		return true;
	}

	return false;
}

static int
tokenizer_add_token(Tokenizer *tokenizer, Token **tokens, size_t *capacity, size_t *count, TokenKind kind)
{
	if (*count >= *capacity && tokenizer_grow_tokens(tokens, capacity) != EXIT_SUCCESS)
	{
		return EXIT_FAILURE;
	}

	(*tokens)[*count] = (Token) {
		.lexeme = tokenizer->start,
		.length = (size_t) (tokenizer->current - tokenizer->start),
		.kind = kind,
		.location = {
			.line = tokenizer->start_line,
			.column = tokenizer->start_column
		}
	};

	(*count)++;
	return EXIT_SUCCESS;
}

static bool
tokenizer_is_identifier_start(char character)
{
	return (character >= 'a' && character <= 'z') || (character >= 'A' && character <= 'Z') || character == '_' || character == '@';
}

static bool
tokenizer_is_digit(char character)
{
	return character >= '0' && character <= '9';
}

static bool
tokenizer_is_binary_digit(char character)
{
	return character == '0' || character == '1';
}

static bool
tokenizer_is_octal_digit(char character)
{
	return character >= '0' && character <= '7';
}

static bool
tokenizer_is_hex_digit(char character)
{
	return tokenizer_is_digit(character)
		|| (character >= 'a' && character <= 'f')
		|| (character >= 'A' && character <= 'F');
}

static bool
tokenizer_is_identifier_char(char character)
{
	return tokenizer_is_identifier_start(character) || tokenizer_is_digit(character);
}

static char
tokenizer_lowercase(char character)
{
	if (character >= 'A' && character <= 'Z')
	{
		return (char) (character + 'a' - 'A');
	}

	return character;
}

static bool
tokenizer_keyword_kind(const char *lexeme, size_t length, TokenKind *kind)
{
	size_t keyword_count = sizeof(tokenizer_keywords) / sizeof(tokenizer_keywords[0]);

	for (size_t i = 0; i < keyword_count; i++)
	{
		const TokenizerKeyword *keyword = &tokenizer_keywords[i];

		if (strlen(keyword->word) != length)
		{
			continue;
		}

		bool match = true;

		for (size_t j = 0; j < length; j++)
		{
			if (tokenizer_lowercase(lexeme[j]) != keyword->word[j])
			{
				match = false;
				break;
			}
		}

		if (match)
		{
			*kind = keyword->kind;
			return true;
		}
	}

	return false;
}

static bool
tokenizer_consume_digits(Tokenizer *tokenizer, bool (*is_digit)(char), bool require_digit)
{
	bool seen_digit = !require_digit;
	bool trailing_underscore = false;

	while (true)
	{
		char character = tokenizer_peek(tokenizer);

		if (character == '_')
		{
			if (!seen_digit)
			{
				return false;
			}

			trailing_underscore = true;
		}
		else if (is_digit(character))
		{
			seen_digit = true;
			trailing_underscore = false;
		}
		else
		{
			break;
		}

		tokenizer_advance(tokenizer);
	}

	if (!seen_digit || trailing_underscore)
	{
		return false;
	}

	return true;
}

static bool
tokenizer_is_escape_char(char character)
{
	switch (character)
	{
		case 'n':
		case 't':
		case 'r':
		case '0':
		case '\\':
		case '\'':
		case '"':
			return true;

		default:
			return false;
	}
}

static int
tokenizer_tokenize_number(Tokenizer *tokenizer, Token **tokens, size_t *capacity, size_t *count)
{
	TokenKind kind = TOKEN_INTEGER;

	// The first digit has already been consumed by the caller.
	if (*tokenizer->start == '0')
	{
		char prefix = tokenizer_peek(tokenizer);

		if (prefix == 'b' || prefix == 'B' || prefix == 'o' || prefix == 'O' || prefix == 'x' || prefix == 'X')
		{
			bool (*is_digit)(char);

			switch (prefix)
			{
				case 'b':
				case 'B':
					kind = TOKEN_BINARY_INTEGER;
					is_digit = tokenizer_is_binary_digit;
					break;

				case 'o':
				case 'O':
					kind = TOKEN_OCTAL_INTEGER;
					is_digit = tokenizer_is_octal_digit;
					break;

				default:
					kind = TOKEN_HEX_INTEGER;
					is_digit = tokenizer_is_hex_digit;
					break;
			}

			tokenizer_advance(tokenizer);

			if (!tokenizer_consume_digits(tokenizer, is_digit, true)
				|| tokenizer_is_identifier_char(tokenizer_peek(tokenizer)))
			{
				tokenizer_error("invalid number literal at %d:%d", tokenizer->start_line, tokenizer->start_column);
				return EXIT_FAILURE;
			}

			return tokenizer_add_token(tokenizer, tokens, capacity, count, kind);
		}
	}

	if (!tokenizer_consume_digits(tokenizer, tokenizer_is_digit, false))
	{
		tokenizer_error("invalid number literal at %d:%d", tokenizer->start_line, tokenizer->start_column);
		return EXIT_FAILURE;
	}

	if (tokenizer_peek(tokenizer) == '.' && tokenizer_is_digit(tokenizer_peek_next(tokenizer)))
	{
		tokenizer_advance(tokenizer);

		if (!tokenizer_consume_digits(tokenizer, tokenizer_is_digit, true))
		{
			tokenizer_error("invalid number literal at %d:%d", tokenizer->start_line, tokenizer->start_column);
			return EXIT_FAILURE;
		}

		kind = TOKEN_FLOAT;
	}

	char exponent = tokenizer_peek(tokenizer);

	if (exponent == 'e' || exponent == 'E')
	{
		char sign = tokenizer_peek_next(tokenizer);
		bool has_digits = tokenizer_is_digit(sign);

		if (!has_digits && (sign == '+' || sign == '-'))
		{
			has_digits = tokenizer_is_digit(tokenizer->current[2]);
		}

		if (has_digits)
		{
			tokenizer_advance(tokenizer);

			if (sign == '+' || sign == '-')
			{
				tokenizer_advance(tokenizer);
			}

			if (!tokenizer_consume_digits(tokenizer, tokenizer_is_digit, true))
			{
				tokenizer_error("invalid number literal at %d:%d", tokenizer->start_line, tokenizer->start_column);
				return EXIT_FAILURE;
			}

			kind = TOKEN_SCIENTIFIC_FLOAT;
		}
	}

	return tokenizer_add_token(tokenizer, tokens, capacity, count, kind);
}

static int
tokenizer_tokenize_string(Tokenizer *tokenizer, Token **tokens, size_t *capacity, size_t *count)
{
	while (true)
	{
		char character = tokenizer_peek(tokenizer);

		if (character == '\0')
		{
			tokenizer_error("unterminated string at %d:%d", tokenizer->start_line, tokenizer->start_column);
			return EXIT_FAILURE;
		}

		if (character == '\\')
		{
			char escape = tokenizer_peek_next(tokenizer);

			if (escape == '\0')
			{
				tokenizer_error("unterminated string at %d:%d", tokenizer->start_line, tokenizer->start_column);
				return EXIT_FAILURE;
			}

			if (!tokenizer_is_escape_char(escape))
			{
				tokenizer_error("invalid escape sequence `\\%c` in string at %d:%d", escape, tokenizer->start_line, tokenizer->start_column);
				return EXIT_FAILURE;
			}

			tokenizer_advance(tokenizer);
			tokenizer_advance(tokenizer);
			continue;
		}

		tokenizer_advance(tokenizer);

		if (character == '"')
		{
			break;
		}
	}

	return tokenizer_add_token(tokenizer, tokens, capacity, count, TOKEN_STRING);
}

static int
tokenizer_tokenize_rune(Tokenizer *tokenizer, Token **tokens, size_t *capacity, size_t *count)
{
	char character = tokenizer_peek(tokenizer);

	if (character == '\\')
	{
		char escape = tokenizer_peek_next(tokenizer);

		if (escape == '\0')
		{
			tokenizer_error("unterminated rune at %d:%d", tokenizer->start_line, tokenizer->start_column);
			return EXIT_FAILURE;
		}

		if (!tokenizer_is_escape_char(escape))
		{
			tokenizer_error("invalid escape sequence `\\%c` in rune at %d:%d", escape, tokenizer->start_line, tokenizer->start_column);
			return EXIT_FAILURE;
		}

		tokenizer_advance(tokenizer);
	}
	else if (character == '\'' || character == '\0')
	{
		tokenizer_error("empty or unterminated rune at %d:%d", tokenizer->start_line, tokenizer->start_column);
		return EXIT_FAILURE;
	}

	tokenizer_advance(tokenizer);

	if (!tokenizer_match(tokenizer, '\''))
	{
		tokenizer_error("invalid rune literal at %d:%d", tokenizer->start_line, tokenizer->start_column);
		return EXIT_FAILURE;
	}

	return tokenizer_add_token(tokenizer, tokens, capacity, count, TOKEN_RUNE);
}

void
tokenizer_free(Token **tokens)
{
	if (tokens != NULL)
	{
		free(*tokens);
		*tokens = NULL;
	}
}

void
tokenizer_print(const Token *tokens, size_t count)
{
	for (size_t i = 0; i < count; i++)
	{
		const Token *token = &tokens[i];
		printf("%d:%d kind=%s lexem='%.*s'\n", token->location.line, token->location.column, tokenizer_kind_name(token->kind), (int) token->length, token->lexeme);
	}
}


int
tokenizer_tokenize(const char *source, Token **tokens, size_t *count)
{
	Tokenizer tokenizer = {
		.start = source,
		.current = source,
		.line = 1,
		.column = 1
	};

	*tokens = NULL;
	*count = 0;

	size_t capacity = 0;

	while (tokenizer_peek(&tokenizer) != '\0')
	{
		tokenizer.start = tokenizer.current;
		tokenizer.start_line = tokenizer.line;
		tokenizer.start_column = tokenizer.column;

		char character = tokenizer_advance(&tokenizer);

		if (tokenizer_is_identifier_start(character))
		{
			while (tokenizer_is_identifier_char(tokenizer_peek(&tokenizer)))
			{
				tokenizer_advance(&tokenizer);
			}

			size_t length = (size_t) (tokenizer.current - tokenizer.start);
			TokenKind kind = TOKEN_IDENTIFIER;

			if (character == '@')
			{
				kind = TOKEN_AT;
			}
			else if (character == '_' && length == 1)
			{
				kind = TOKEN_UNDERSCORE;
			}
			else
			{
				TokenKind keyword_kind;

				if (tokenizer_keyword_kind(tokenizer.start, length, &keyword_kind))
				{
					kind = keyword_kind;
				}
			}

			if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, kind) != EXIT_SUCCESS)
			{
				goto error;
			}

			continue;
		}

		if (tokenizer_is_digit(character))
		{
			if (tokenizer_tokenize_number(&tokenizer, tokens, &capacity, count) != EXIT_SUCCESS)
			{
				goto error;
			}

			continue;
		}

		switch (character)
		{
			case ' ':
			case '\t':
			case '\r':
			case '\n':
				break;

			case '#':
				tokenizer_skip_line(&tokenizer);
				break;

			case '(':
				if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_LEFT_PAREN) != EXIT_SUCCESS)
				{
					goto error;
				}
				break;

			case ')':
				if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_RIGHT_PAREN) != EXIT_SUCCESS)
				{
					goto error;
				}
				break;

			case '{':
				if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_LEFT_CURLY) != EXIT_SUCCESS)
				{
					goto error;
				}
				break;

			case '}':
				if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_RIGHT_CURLY) != EXIT_SUCCESS)
				{
					goto error;
				}
				break;

			case '[':
				if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_LEFT_SQUARE) != EXIT_SUCCESS)
				{
					goto error;
				}
				break;

			case ']':
				if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_RIGHT_SQUARE) != EXIT_SUCCESS)
				{
					goto error;
				}
				break;

			case '%':
				if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_PERCENT) != EXIT_SUCCESS)
				{
					goto error;
				}
				break;

			case '*':
				if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_ASTERISK) != EXIT_SUCCESS)
				{
					goto error;
				}
				break;

			case '^':
				if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_CARET) != EXIT_SUCCESS)
				{
					goto error;
				}
				break;

			case '~':
				if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_TILDE) != EXIT_SUCCESS)
				{
					goto error;
				}
				break;

			case '|':
				if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_PIPE) != EXIT_SUCCESS)
				{
					goto error;
				}
				break;

			case '.':
				if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_DOT) != EXIT_SUCCESS)
				{
					goto error;
				}
				break;

			case '+':
				if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_PLUS) != EXIT_SUCCESS)
				{
					goto error;
				}
				break;

			case '-':
				if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_MINUS) != EXIT_SUCCESS)
				{
					goto error;
				}
				break;

			case '/':
				if (tokenizer_match(&tokenizer, '/'))
				{
					if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_SLASH_SLASH) != EXIT_SUCCESS)
					{
						goto error;
					}
				}
				else
				{
					if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_SLASH) != EXIT_SUCCESS)
					{
						goto error;
					}
				}
				break;

			case '=':
				if (tokenizer_match(&tokenizer, '='))
				{
					if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_EQUAL_EQUAL) != EXIT_SUCCESS)
					{
						goto error;
					}
				}
				else
				{
					if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_EQUAL) != EXIT_SUCCESS)
					{
						goto error;
					}
				}
				break;

			case '!':
				if (tokenizer_match(&tokenizer, '='))
				{
					if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_NOT_EQUAL) != EXIT_SUCCESS)
					{
						goto error;
					}
				}
				else
				{
					if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_BANG) != EXIT_SUCCESS)
					{
						goto error;
					}
				}
				break;

			case '<':
				if (tokenizer_match(&tokenizer, '='))
				{
					if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_LESS_EQUAL) != EXIT_SUCCESS)
					{
						goto error;
					}
				}
				else if (tokenizer_match(&tokenizer, '<'))
				{
					if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_LEFT_SHIFT) != EXIT_SUCCESS)
					{
						goto error;
					}
				}
				else
				{
					if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_LESS) != EXIT_SUCCESS)
					{
						goto error;
					}
				}
				break;

			case '>':
				if (tokenizer_match(&tokenizer, '='))
				{
					if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_GREATER_EQUAL) != EXIT_SUCCESS)
					{
						goto error;
					}
				}
				else if (tokenizer_match(&tokenizer, '>'))
				{
					if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_RIGHT_SHIFT) != EXIT_SUCCESS)
					{
						goto error;
					}
				}
				else
				{
					if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_GREATER) != EXIT_SUCCESS)
					{
						goto error;
					}
				}
				break;

			case '"':
				if (tokenizer_tokenize_string(&tokenizer, tokens, &capacity, count) != EXIT_SUCCESS)
				{
					goto error;
				}
				break;

			case '\'':
				if (tokenizer_tokenize_rune(&tokenizer, tokens, &capacity, count) != EXIT_SUCCESS)
				{
					goto error;
				}
				break;

			default:
				tokenizer_error("unknown lexem `%c`", character);
				break;
		}
	}

	tokenizer.start = tokenizer.current;
	tokenizer.start_line = tokenizer.line;
	tokenizer.start_column = tokenizer.column;

	if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_EOF) != EXIT_SUCCESS)
	{
		goto error;
	}

	return EXIT_SUCCESS;

	error:
	free(*tokens);
	*tokens = NULL;
	*count = 0;
	return EXIT_FAILURE;
}
