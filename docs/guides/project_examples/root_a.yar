# Project root A. Sibling of root_b; both use shared.util.
# Checked via `yarrow check` (CLI Stage 13) / yarrow_core check_project (Stage 28).

"shared.util" util require

main function do
	util.ping call
	drop
end
