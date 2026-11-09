#include <assert.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "cli.h"

int
cli_run_command(const CliParseResult *result)
{
	assert(0 && "Not implemented");
	return -1;
}

int
cli_build_command(const CliParseResult *result)
{
	assert(0 && "Not implemented");
	return -1;
}

int
cli_interpret_command(const CliParseResult *result)
{
	assert(0 && "Not implemented");
	return -1;
}

int
cli_lsp_command(const CliParseResult *result)
{
	assert(0 && "Not implemented");
	return -1;
}

int
cli_fmt_command(const CliParseResult *result)
{
	assert(0 && "Not implemented");
	return -1;
}

int
cli_debug_command(const CliParseResult *result)
{
	assert(0 && "Not implemented");
	return -1;
}

int
cli_debug_ast_command(const CliParseResult *result)
{
	assert(0 && "Not implemented");
	return -1;
}

int
cli_debug_ir_command(const CliParseResult *result)
{
	assert(0 && "Not implemented");
	return -1;
}

int
cli_debug_tokens_command(const CliParseResult *result)
{
	assert(0 && "Not implemented");
	return -1;
}

static CliCommand *
cli_find_command(CliCommand *commands, size_t count, const char *name)
{
	for (size_t i = 0; i < count; i++)
	{
		if (strcmp(commands[i].name, name) == 0)
		{
			return &commands[i];
		}
	}

	return NULL;
}

static CliFlag *
cli_find_flag(CliFlag *flags, size_t count, const char *name)
{
	for (size_t i = 0; i < count; i++)
	{
		if (strcmp(flags[i].name, name) == 0 || (flags[i].short_name != NULL && strcmp(flags[i].short_name, name) == 0))
		{
			return &flags[i];
		}
	}

	return NULL;
}

static CliFlag *
cli_resolve_flag(Cli *cli, CliCommand **commands, size_t command_count, const char *name)
{
	for (size_t i = command_count; i > 0; i--)
	{
		CliCommand *command = commands[i - 1];
		CliFlag *flag = cli_find_flag(command->flags, command->flag_count, name);

		if (flag != NULL)
		{
			return flag;
		}
	}

	return cli_find_flag(cli->global_flags, cli->global_flag_count, name);
}

int
cli_parse(Cli *cli, int argc, char **argv, CliParseResult *result)
{
	if (cli == NULL || result == NULL || argc < 1 || argv == NULL)
	{
		return -1;
	}

	memset(result, 0, sizeof(*result));

	result->flags = calloc((size_t) argc, sizeof(CliParseResult));
	result->positionals = calloc((size_t) argc, sizeof(char *));

	CliCommand **commands = calloc((size_t) argc, sizeof(CliCommand *));

	if (result->flags == NULL || result->positionals == NULL || commands == NULL)
	{
		goto error;
	}

	size_t command_count = 0;
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
					size_t lenght = (size_t)(equals - name);
					allocated_name = malloc(lenght + 1);

					if (allocated_name == NULL)
					{
						goto error;
					}

					memcpy(allocated_name, name, lenght);
					allocated_name[lenght] = '\0';

					name = allocated_name;
					value = equals + 1;
				}
			}
			else
			{
				name = arg + 1;
			}

			if (*name == '\0')
			{
				free(allocated_name);
				goto error;
			}

			CliFlag *flag = cli_resolve_flag(cli, commands, command_count, name);
			free(allocated_name);

			if (flag == NULL)
			{
				printf("ERROR: unknown option %s\n", arg);
				goto error;
			}

			if (flag->takes_value == CLI_FLAG_REQUIRED_VALUE)
			{
				if (value == NULL)
				{
					if (i + 1 >= argc)
					{
						printf("ERROR: option %s require a value", arg);
						goto error;
					}

					value = argv[++i];
				}
			}

			if (flag->takes_value == CLI_FLAG_OPTIONAL_VALUE)
			{
				assert(0 && "Not implemented");
				return -1;
			}

			result->flags[result->flag_count++] = (CliParsedFlag) {
				.flag = flag,
				.value = value,
				.present = 1
			};

			continue;
		}

		if (command_count == 0)
		{
			CliCommand *command = cli_find_command(cli->commands, cli->command_count, arg);
			if (command == NULL)
			{
				printf("ERROR: unknown command %s\n", arg);
				goto error;
			}

			commands[command_count++] = command;
			continue;
		}

		CliCommand *current = commands[command_count - 1];
		CliCommand *subcommand = cli_find_command(current->subcommands, current->subcommand_count, arg);
		if (subcommand != NULL)
		{
			commands[command_count++] = subcommand;
			continue;
		}

		result->positionals[result->positional_count++] = arg;
	}

	if (command_count > 0)
	{
		result->command = commands[command_count - 1];
	}

	free(commands);
	return 0;

	error:
	free(commands);
	cli_parse_free(result);
	return -1;
}

int
cli_parse_free(CliParseResult *result)
{
	if (result == NULL)
	{
		return 0;
	}

	free(result->flags);
	free(result->positionals);

	memset(result, 0, sizeof(*result));

	return 0;
}

int
cli_help(Cli *cli, CliCommand *command)
{
	if (command != NULL)
	{
		assert(0 && "Not implemented: command specific help");
		return -1;
	}

	if (cli == NULL)
	{
		return -1;
	}

	if (cli->command_count > 0 && cli->commands == NULL)
	{
		return -1;
	}

	if (cli->global_flag_count > 0 && cli->global_flags == NULL)
	{
		return -1;
	}

	printf("Usage: %s [options] file\n", cli->name);
	printf("\n");

	printf("Commands:\n");
	for (int i = 0; i < cli->command_count; i++)
	{
		printf("\t%-20s %s\n", cli->commands[i].name, cli->commands[i].description);
	}
	printf("\n");

	printf("Options:\n");
	for (int i = 0; i < cli->global_flag_count; i++)
	{
		printf("\t--%-10s -%-6s %s\n", cli->global_flags[i].name, cli->global_flags[i].short_name, cli->global_flags[i].description);
	}
	printf("\n");

	return 0;
}

int
cli_init(Cli *cli)
{
	static CliCommand debug_subcommands[] = {
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
		}
	};

	static CliCommand commands[] = {
		{
			.name = "run",
			.description = "Build and run an executable",
			.handler = cli_run_command
		},
		{
			.name = "build",
			.description = "Build an executable",
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
			.handler = cli_debug_command,
			.subcommands = debug_subcommands,
			.subcommand_count = sizeof(debug_subcommands) / sizeof(debug_subcommands[0])
		}
	};

	static CliFlag global_flags[] = {
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
		}
	};

	cli->name = "yarrow";
	cli->description = "";
	cli->commands = commands;
	cli->command_count = sizeof(commands) / sizeof(commands[0]);
	cli->global_flags = global_flags;
	cli->global_flag_count = sizeof(global_flags) / sizeof(global_flags[0]);

	return 0;
}

int
cli_run(int argc, char **argv)
{
	Cli cli;
	if (cli_init(&cli) != 0)
	{
		return -1;
	}

	CliParseResult result;
	if (cli_parse(&cli, argc, argv, &result) != 0)
	{
		return -1;
	}

	int status = 0;
	if (result.command != NULL && result.command->handler != NULL)
	{
		status = result.command->handler(&result);
	}
	else
	{
		cli_help(&cli, result.command);
	}

	cli_parse_free(&result);
	return status;
}
