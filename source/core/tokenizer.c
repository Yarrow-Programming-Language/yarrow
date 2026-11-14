#include <assert.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>

#include "tokenizer.h"

// Error function with arguments check safe-guard

#if defined(__GNUC__)
__attribute__((format(printf, 1, 2)))
#endif
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

// Some helper functions

static int
tokenizer_grow(Tokenizer *tokenizer, Token **tokens, size_t *capacity, size_t count)
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
	if (*count >= *capacity && tokenizer_grow(tokenizer, tokens, capacity, *count) != EXIT_SUCCESS)
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

// All the important stuff

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
			// Check for any identifier not defined as a keyword
			assert(0 && "Not implemented");
			continue;
		}

		if (tokenizer_is_digit(character))
		{
			// Check for `+`, '-', integers, float
			assert(0 && "Not implemented");
			continue;
		}

		switch (character)
		{
			case ' ':
			case '\t':
			case '\r':
				break;

			case '\n':
				break;

			case '#':
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
				break;

			case '\'':
				break;

			default:
				tokenizer_error("unknown lexem `%c`", character);
				break;
		}
	}

	if (tokenizer_add_token(&tokenizer, tokens, &capacity, count, TOKEN_EOF) != EXIT_SUCCESS)
	{
		goto error;
	}

	return EXIT_SUCCESS;

	error:
	free(tokens);
	*tokens = NULL;
	*count = 0;
	return EXIT_FAILURE;
}
