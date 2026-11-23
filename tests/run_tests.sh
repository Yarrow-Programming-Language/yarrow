#!/usr/bin/env bash
# Fixture runner for the tokenizer and the parser.
#
# Fixture layout:
#   tests/tokenizer/<name>.yar      input, expected output in <name>.tokens
#   tests/parser/valid/<name>.yar   input, expected `debug ast` output in <name>.ast
#   tests/parser/invalid/<name>.yar input, expected `debug ast` error in <name>.err
#
# The grammar tour fixture uses docs/examples/00_grammar_tour.yar as input and
# tests/parser/valid/grammar_tour.ast as expected output.
#
# Expected outputs embed the fixture paths, so the runner always invokes the
# binary with paths relative to the repository root.

set -u

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

binary="./build/yarrow"
workdir=".tmp/tests"

passed=0
failed=0

report()
{
	local status="$1"
	local fixture="$2"

	if [ "$status" = "ok" ]; then
		passed=$((passed + 1))
		printf 'ok   %s\n' "$fixture"
	else
		failed=$((failed + 1))
		printf 'FAIL %s\n' "$fixture"
	fi
}

# run_fixture <mode> <input> <expected> <expected-status>
# mode is one of: tokens, ast, error
run_fixture()
{
	local mode="$1"
	local input="$2"
	local expected="$3"
	local expected_status="$4"

	local subcommand="ast"
	[ "$mode" = "tokens" ] && subcommand="tokens"

	mkdir -p "$workdir"
	local stdout="$workdir/stdout"
	local stderr="$workdir/stderr"

	"$binary" debug "$subcommand" --input "$input" >"$stdout" 2>"$stderr"
	local status=$?

	local output="$stdout"
	[ "$mode" = "error" ] && output="$stderr"

	if [ "$expected_status" = "zero" ] && [ "$status" -ne 0 ]; then
		report "fail" "$input (expected success, got exit $status)"
		return
	fi
	if [ "$expected_status" = "nonzero" ] && [ "$status" -eq 0 ]; then
		report "fail" "$input (expected failure, got success)"
		return
	fi
	if ! diff -u "$expected" "$output" >"$workdir/diff" 2>&1; then
		cat "$workdir/diff"
		report "fail" "$input (output mismatch)"
		return
	fi

	report "ok" "$input"
}

if [ ! -x "$binary" ]; then
	echo "missing binary $binary, run make build first" >&2
	exit 1
fi

for input in tests/tokenizer/*.yar; do
	[ -e "$input" ] || continue
	run_fixture tokens "$input" "${input%.yar}.tokens" zero
done

for input in tests/parser/valid/*.yar; do
	[ -e "$input" ] || continue
	run_fixture ast "$input" "${input%.yar}.ast" zero
done

if [ -f docs/examples/00_grammar_tour.yar ]; then
	run_fixture ast docs/examples/00_grammar_tour.yar tests/parser/valid/grammar_tour.ast zero
fi

for input in tests/parser/invalid/*.yar; do
	[ -e "$input" ] || continue
	run_fixture error "$input" "${input%.yar}.err" nonzero
done

rm -rf "$workdir"

printf '\n%d passed, %d failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
