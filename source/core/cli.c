/*
 * This file define the Yarrow cli.
 * It is the main entry point for using the language.
 * It's nothing complicated.
 *
 * The structure of the cli is as follow:
 * Cli                    The main structure
 * | -> Commands          The commands defined in the cli (e.g build, run, init...)
 * | | -> Subcommands     Subcommands of the parent command
 * | | -> Options         Flags of the parent command
 * | -> Global Options    The flags that take priority over all other flags
 *
 * Except for the help global option that work for specific commands to get their
 * specific help, nothing else should be more complicated than this.
 */

#include <stdio.h>
#include <assert.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <stdbool.h>

#include "cli.h"
#include "fmt.h"
#include "lsp.h"

// Error function with arguments check safe-guard

#if defined(__GNUC__)
__attribute__((format(printf, 1, 2)))
#endif
static void
cli_error(const char *format, ...)
{
	va_list args;

	va_start(args, format);
	fprintf(stderr, "ERROR: ");
	vfprintf(stderr, format, args);
	fprintf(stderr, "\n");
	va_end(args);
}

// Some helper functions

static void
cli_help_commands(const CliCommand *commands)
{
	for (const CliCommand *command = commands; command->name != NULL; command++)
	{
		printf("  ");
		printf("%-20s", command->name);
		printf("%s\n", command->description);
	}
}

static void
cli_help_flags(const CliFlag *flags)
{
	for (const CliFlag *flag = flags; flag->name != NULL; flag++)
	{
		printf("  ");

		if (flag->short_name != NULL)
		{
			printf("-%s, ", flag->short_name);
		}
		else
		{
			printf("    ");
		}

		printf("--%-14s", flag->name);

		if (flag->takes_value == CLI_FLAG_REQUIRED_VALUE)
		{
			printf(" <%s>", flag->value_name != NULL ? flag->value_name : "value");
		}
		else if (flag->takes_value == CLI_FLAG_OPTIONAL_VALUE)
		{
			printf(" [%s]", flag->value_name != NULL ? flag->value_name : "value");
		}

		printf("%s\n", flag->description);
	}
}

static int
cli_help(const Cli *cli, const CliParsedResult *result)
{
	if (cli == NULL || result == NULL)
	{
		return EXIT_FAILURE;
	}

	if (result->command != NULL)
	{
		printf("Usage: %s", cli->name);

		for (CliCommand **path = result->command_path; *path != NULL; path++)
		{
			printf(" %s", (*path)->name);
		}

		if (result->command->subcommands != NULL && result->command->subcommands[0].name != NULL)
		{
			printf(" [commands]");
		}

		if (result->command->flags != NULL && result->command->flags[0].name != NULL)
		{
			printf(" [options]");
		}

		printf("\n");

		if (result->command->description != NULL && strcmp(result->command->description, "") != 0)
		{
			printf("\n");
			printf("%s\n", result->command->description);
		}

		printf("\n");

		if (result->command->subcommands != NULL && result->command->subcommands[0].name != NULL)
		{
			printf("Commands:\n");
			cli_help_commands(result->command->subcommands);
			printf("\n");
		}

		if (result->command->flags != NULL && result->command->flags[0].name != NULL)
		{
			printf("Options:\n");
			cli_help_flags(result->command->flags);
			printf("\n");
		}
	}
	else
	{
		printf("Usage: %s [command] [options]\n", cli->name);
		printf("\n");
		printf("Commands:\n");
		cli_help_commands(cli->commands);
		printf("\n");
	}

	printf("Global options:\n");
	cli_help_flags(cli->global_flags);

	return EXIT_SUCCESS;
}

static bool
cli_has_flag(const CliParsedResult *result, const char *name)
{
	for (const CliParsedFlag *flag = result->flags; flag->flag != NULL; flag++)
	{
		if (flag->present && strcmp(flag->flag->name, name) == 0)
		{
			return true;
		}
	}

	return false;
}

static const char *
cli_get_flag_value(const CliParsedResult *result, const char *name)
{
	for (const CliParsedFlag *flag = result->flags; flag->flag != NULL; flag++)
	{
		if (flag->present && strcmp(flag->flag->name, name) == 0)
		{
			return flag->value;
		}
	}

	return NULL;
}

static bool
cli_has_global_flags(const CliParsedResult *result)
{
	return cli_has_flag(result, "help") || cli_has_flag(result, "version");
}

static bool
cli_execute_global_flags(const Cli *cli, const CliParsedResult *result)
{
	if (cli_has_flag(result, "help"))
	{
		if (cli_help(cli, result) != 0)
		{
			return false;
		}

		return true;
	}

	// Should also print the tokenizer, interpreter, ast and compiler version
	if (cli_has_flag(result, "version"))
	{
		printf("Cli version: %s\nFmt version: %s\nLsp version: %s\n", CLI_VERSION, FMT_VERSION, LSP_VERSION);
		return true;
	}

	return false;
}

static CliCommand *
cli_find_command(CliCommand *commands, const char *name)
{
	for (CliCommand *command = commands; command->name != NULL; command++)
	{
		if (strcmp(command->name, name) == 0)
		{
			return command;
		}
	}

	return NULL;
}

static CliFlag *
cli_find_flag(CliFlag *flags, const char *name)
{
	for (CliFlag *flag = flags; flag->name != NULL; flag++)
	{
		if (strcmp(flag->name, name) == 0 || (flag->short_name != NULL && strcmp(flag->short_name, name) == 0))
		{
			return flag;
		}
	}

	return NULL;
}

static CliFlag *
cli_resolve_flag(const Cli *cli, CliCommand **commands, const char *name)
{
	CliCommand **end = commands;

	while (*end != NULL)
	{
		end++;
	}

	for (CliCommand **it = end; it > commands; it--)
	{
		CliFlag *flag = cli_find_flag((*(it - 1))->flags, name);

		if (flag != NULL)
		{
			return flag;
		}
	}

	return cli_find_flag(cli->global_flags, name);
}

static void
cli_parse_free(CliParsedResult *result)
{
	if (result != NULL)
	{
		free(result->flags);
		free(result->positionals);
		free(result->command_path);

		memset(result, 0, sizeof(*result));
	}
}

// Below are the handling functions for commands

int
cli_run_command(const CliParsedResult *result)
{
	assert(0 && "Not implemented");
	return EXIT_FAILURE;
}

int
cli_init_command(const CliParsedResult *result)
{
	int exe = cli_has_flag(result, "exe");
	if (exe)
	{
		assert(0 && "Not implemented");
		return EXIT_FAILURE;
	}

	int lib = cli_has_flag(result, "lib");
	if (lib)
	{
		assert(0 && "Not implemented");
		return EXIT_FAILURE;
	}

	assert(0 && "Not implemented");
	return EXIT_FAILURE;
}

int
cli_build_command(const CliParsedResult *result)
{
	const char *output = cli_get_flag_value(result, "output");
	if (output != NULL)
	{
		assert(0 && "Not implemented");
		return EXIT_FAILURE;
	}

	assert(0 && "Not implemented");
	return EXIT_FAILURE;
}

int
cli_interpret_command(const CliParsedResult *result)
{
	assert(0 && "Not implemented");
	return EXIT_FAILURE;
}

int
cli_lsp_command(const CliParsedResult *result)
{
	assert(0 && "Not implemented");
	return EXIT_FAILURE;
}

int
cli_fmt_command(const CliParsedResult *result)
{
	assert(0 && "Not implemented");
	return EXIT_FAILURE;
}

int
cli_debug_command(const CliParsedResult *result)
{
	assert(0 && "Not implemented");
	return EXIT_FAILURE;
}

int
cli_debug_ast_command(const CliParsedResult *result)
{
	assert(0 && "Not implemented");
	return EXIT_FAILURE;
}

int
cli_debug_ir_command(const CliParsedResult *result)
{
	assert(0 && "Not implemented");
	return EXIT_FAILURE;
}

int
cli_debug_tokens_command(const CliParsedResult *result)
{
	assert(0 && "Not implemented");
	return EXIT_FAILURE;
}

// All the important stuff

static int
cli_parse(const Cli *cli, int argc, char **argv, CliParsedResult *result)
{
	if (cli == NULL || result == NULL || argc < 1 || argv == NULL)
	{
		return EXIT_FAILURE;
	}

	memset(result, 0, sizeof(*result));

	// The buffers can hold argc - 1 entries plus the NULL terminator
	result->flags = calloc((size_t) argc, sizeof(CliParsedFlag));
	result->positionals = calloc((size_t) argc, sizeof(char *));

	CliCommand **commands = calloc((size_t) argc, sizeof(CliCommand *));

	if (result->flags == NULL || result->positionals == NULL || commands == NULL)
	{
		// Diabolical use of the horrendous goto
		goto error;
	}

	size_t command_count = 0;
	size_t flag_count = 0;
	size_t positional_count = 0;
	int options_enabled = 1;

	for (int i = 1; i < argc; i++)
	{
		char *arg = argv[i];

		if (options_enabled && strcmp(arg, "--") == 0)
		{
			options_enabled = 0;
			continue;
		}

		if (options_enabled && arg[0] == '-' && arg[1] != '\0')
		{
			const char *name;
			const char *value = NULL;
			char *allocated_name = NULL;

			int is_long = arg[1] == '-';
			if (is_long)
			{
				name = arg + 2;

				const char *equals = strchr(name, '=');
				if (equals != NULL)
				{
					size_t length = (size_t)(equals - name);
					allocated_name = malloc(length + 1);

					if (allocated_name == NULL)
					{
						goto error;
					}

					memcpy(allocated_name, name, length);
					allocated_name[length] = '\0';

					name = allocated_name;
					value = equals + 1;
				}
			}
			else
			{
				name = arg + 1;

				if (name[0] != '\0' && name[1] != '\0')
				{
					allocated_name = malloc(2);
					if (allocated_name == NULL)
					{
						goto error;
					}

					allocated_name[0] = name[0];
					allocated_name[1] = '\0';

					value = name + 1;
					name = allocated_name;
				}
			}

			if (*name == '\0')
			{
				free(allocated_name);
				cli_error("empty option name in %s", arg);
				goto error;
			}

			CliFlag *flag = cli_resolve_flag(cli, commands, name);
			free(allocated_name);

			if (flag == NULL)
			{
				cli_error("unknown option %s", arg);
				goto error;
			}

			if (flag->takes_value == CLI_FLAG_NONE && value != NULL)
			{
				cli_error("option --%s does not take a value", flag->name);
				goto error;
			}

			if (flag->takes_value == CLI_FLAG_REQUIRED_VALUE)
			{
				if (value == NULL)
				{
					if (i + 1 >= argc)
					{
						cli_error("option %s requires a value", arg);
						goto error;
					}

					value = argv[++i];
				}
			}

			if (flag->takes_value == CLI_FLAG_OPTIONAL_VALUE)
			{
				if (value == NULL)
				{
					value = flag->default_value;
				}
			}

			result->flags[flag_count++] = (CliParsedFlag) {
				.flag = flag,
				.value = value,
				.present = true
			};

			continue;
		}

		if (command_count == 0)
		{
			CliCommand *command = cli_find_command(cli->commands, arg);
			if (command == NULL)
			{
				cli_error("unknown command %s", arg);
				goto error;
			}

			commands[command_count++] = command;
			continue;
		}

		CliCommand *current = commands[command_count - 1];
		CliCommand *subcommand = cli_find_command(current->subcommands, arg);
		if (subcommand != NULL)
		{
			commands[command_count++] = subcommand;
			continue;
		}

		result->positionals[positional_count++] = arg;
	}

	if (command_count > 0)
	{
		result->command = commands[command_count - 1];
	}

	result->command_path = commands;

	return EXIT_SUCCESS;

	error:
	free(commands);
	cli_parse_free(result);
	return EXIT_FAILURE;
}

int
cli_run(int argc, char **argv)
{
	const Cli cli = {
		.name = "yarrow",
		.description = "",
		.commands = (CliCommand[]) {
			{
				.name = "run",
				.description = "Build and run an executable",
				.handler = cli_run_command
			},
			{
				.name = "init",
				.description = "Initialize a Yarrow project",
				.flags = (CliFlag[]) {
					{
						.name = "exe",
						.short_name = "e",
						.description = "Initialize a Yarrow project for an executable",
						.takes_value = CLI_FLAG_NONE
					},
					{
						.name = "lib",
						.short_name = "l",
						.description = "Initialize a Yarrow project for a library",
						.takes_value = CLI_FLAG_NONE
					},
					{ 0 }
				},
				.handler = cli_init_command
			},
			{
				.name = "build",
				.description = "Build an executable",
				.flags = (CliFlag[]) {
					{
						.name = "output",
						.short_name = "o",
						.description = "File path to output executable",
						.takes_value = CLI_FLAG_REQUIRED_VALUE,
						.value_name = "file"
					},
					{ 0 }
				},
				.handler = cli_build_command
			},
			{
				.name = "interpret",
				.description = "Interpret without building an executable",
				.handler = cli_interpret_command
			},
			{
				.name = "lsp",
				.description = "Run the language server protocol for Yarrow",
				.handler = cli_lsp_command
			},
			{
				.name = "fmt",
				.description = "Run the formatter of Yarrow source code",
				.handler = cli_fmt_command
			},
			{
				.name = "debug",
				.description = "Helper command for Yarrow development",
				.subcommands = (CliCommand[]) {
					{
						.name = "ast",
						.description = "Print the current program AST",
						.handler = cli_debug_ast_command
					},
					{
						.name = "ir",
						.description = "Print the current program IR",
						.handler = cli_debug_ir_command
					},
					{
						.name = "tokens",
						.description = "Print the current program tokens",
						.handler = cli_debug_tokens_command
					},
					{ 0 }
				},
				.handler = cli_debug_command
			},
			{ 0 }
		},
		.global_flags = (CliFlag[]) {
			{
				.name = "version",
				.short_name = "v",
				.description = "Give the current version of Yarrow",
				.takes_value = CLI_FLAG_NONE,
			},
			{
				.name = "help",
				.short_name = "h",
				.description = "Print this or a command specific help",
				.takes_value = CLI_FLAG_NONE,
			},
			{ 0 }
		}
	};

	CliParsedResult result;
	if (cli_parse(&cli, argc, argv, &result) != 0)
	{
		return EXIT_FAILURE;
	}

	int status;
	if (cli_has_global_flags(&result))
	{
		status = cli_execute_global_flags(&cli, &result) ? EXIT_SUCCESS : EXIT_FAILURE;
	}
	else if (result.command != NULL && result.command->handler != NULL)
	{
		status = result.command->handler(&result);
	}
	else
	{
		status = cli_help(&cli, &result);
	}

	cli_parse_free(&result);
	return status;
}
