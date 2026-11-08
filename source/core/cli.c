#include <assert.h>
#include <stdio.h>
#include "cli.h"

int
cli_parse(int argc, char *argv[])
{


	return 0;
}

int
cli_global_help(Cli *cli, CliCommand *command)
{
	if (command != NULL)
	{
		assert(0 && "Not implemented: command specific help");
		return 0;
	}

	if (cli->commands == NULL || cli->global_flags == NULL)
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
	static CliCommand commands[] = {
		{
			.name = "run",
			.description = "Build and run an executable",
		},
		{
			.name = "build",
			.description = "Build an executable",
		},
		{
			.name = "interpret",
			.description = "Interpret without building an executable",
		},
		{
			.name = "lsp",
			.description = "Run the language server protocol for Yarrow",
		},
		{
			.name = "fmt",
			.description = "Run the formatter of Yarrow source code",
		},
		{
			.name = "debug",
			.description = "Helper command for Yarrow development",
		}
	};

	static CliFlag global_flags[] = {
		{
			.name = "version",
			.short_name = "v",
			.description = "Give the current version of Yarrow",
			.takes_value = 0,
		},
		{
			.name = "help",
			.short_name = "h",
			.description = "Print this or a command specific help",
			.takes_value = 0,
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
cli_run(int argc, char *argv[])
{
	Cli cli;
	cli_init(&cli);
	cli_parse(argc, argv);

	return 0;
}
