#include <assert.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "cli.h"
#include "fmt.h"
#include "lsp.h"

int
cli_run_command(const CliParseResult *result)
{
	assert(0 && "Not implemented");
	return -1;
}

int
cli_init_command(const CliParseResult *result)
{
	int exe = cli_has_flag(result, "exe");
	if (exe)
	{
		assert(0 && "Not implemented");
		return -1;
	}

	int lib = cli_has_flag(result, "lib");
	if (lib)
	{
		assert(0 && "Not implemented");
		return -1;
	}

	assert(0 && "Not implemented");
	return -1;
}

int
cli_build_command(const CliParseResult *result)
{
	const char *output = cli_get_flag_value(result, "output");
	if (output != NULL)
	{
		assert(0 && "Not implemented");
		return -1;
	}

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

int
cli_execute_global_flags(Cli *cli, CliParseResult *result)
{
	if (cli_has_flag(result, "help"))
	{
		return cli_help(cli, result);
	}

	if (cli_has_flag(result, "version"))
	{
		printf("Cli version: %s\nFmt version: %s\nLsp version: %s\n", CLI_VERSION, FMT_VERSION, LSP_VERSION);
		return 0;
	}

	return -1;
}

int
cli_has_flag(const CliParseResult *result, const char *name)
{
	for (size_t i = 0; i < result->flag_count; i++)
	{
		if (strcmp(result->flags[i].flag->name, name) == 0)
		{
			return 1;
		}
	}

	return 0;
}

const char *
cli_get_flag_value(const CliParseResult *result, const char *name)
{
	for (size_t i = 0; i < result->flag_count; i++)
	{
		if (strcmp(result->flags[i].flag->name, name) == 0)
		{
			return result->flags[i].value;
		}
	}

	return NULL;
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
						printf("ERROR: option %s require a value\n", arg);
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

	result->command_path = commands;
	result->command_depth = command_count;

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
	free(result->command_path);

	memset(result, 0, sizeof(*result));

	return 0;
}

void
cli_help_commands(CliCommand *commands, size_t count)
{
	for (size_t i = 0; i < count; i++)
	{
		CliCommand *command = &commands[i];

		printf("  ");
		printf("%-20s", command->name);
		printf("%s\n", command->description);
	}
}

void
cli_help_flags(CliFlag *flags, size_t count)
{
	for (size_t i = 0; i < count; i++)
	{
		CliFlag *flag = &flags[i];

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

int
cli_help(Cli *cli, CliParseResult *result)
{
	if (cli == NULL)
	{
		return -1;
	}

	if (result->command != NULL)
	{
		printf("Usage: %s", cli->name);

		for (size_t i = 0; i < result->command_depth; i++)
		{
			printf(" %s", result->command_path[i]->name);
		}

		if (result->command->subcommand_count > 0)
		{
			printf(" [commands]");
		}

		if (result->command->flag_count > 0)
		{
			printf(" [options]");
		}

		printf("\n\n");

		if (result->command->description != NULL)
		{
			printf("%s\n", result->command->description);
		}

		printf("\n");

		if (result->command->subcommand_count > 0)
		{
			printf("Commands:\n");
			cli_help_commands(result->command->subcommands, result->command->subcommand_count);
			printf("\n");
		}

		if (result->command->flag_count > 0)
		{
			printf("Options:\n");
			cli_help_flags(result->command->flags, result->command->flag_count);
			printf("\n");
		}
	}
	else
	{
		printf("Usage: %s [command] [options]\n", cli->name);
		printf("\n");
		printf("Commands:\n");
		cli_help_commands(cli->commands, cli->command_count);
		printf("\n");
	}

	printf("Global options:\n");
	cli_help_flags(cli->global_flags, cli->global_flag_count);

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

	static CliFlag init_flags[] = {
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
		}
	};

	static CliFlag build_flags[] = {
		{
			.name = "output",
			.short_name = "o",
			.description = "File path to output executable",
			.takes_value = CLI_FLAG_REQUIRED_VALUE,
			.value_name = "file"
		}
	};

	static CliCommand commands[] = {
		{
			.name = "run",
			.description = "Build and run an executable",
			.handler = cli_run_command
		},
		{
			.name = "init",
			.description = "Initialize a Yarrow project",
			.flags = init_flags,
			.flag_count = sizeof(init_flags) / sizeof(init_flags[0]),
			.handler = cli_init_command
		},
		{
			.name = "build",
			.description = "Build an executable",
			.flags = build_flags,
			.flag_count = sizeof(build_flags) / sizeof(build_flags[0]),
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
			.subcommands = debug_subcommands,
			.subcommand_count = sizeof(debug_subcommands) / sizeof(debug_subcommands[0]),
			.handler = cli_debug_command
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
	int global_flag_status = cli_execute_global_flags(&cli, &result);
	if (global_flag_status != -1)
	{
		status = global_flag_status;
	}
	else if (result.command != NULL && result.command->handler != NULL)
	{
		status = result.command->handler(&result);
	}
	else
	{
		cli_help(&cli, &result);
	}

	cli_parse_free(&result);
	return status;
}
