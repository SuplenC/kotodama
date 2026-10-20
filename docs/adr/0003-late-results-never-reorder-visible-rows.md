# 3. Late Results never reorder the rows already on screen

Status: accepted (2026-10-19), superseded by ADR 0020 (late Results may rerank the rows until the selection moves or 150 ms pass)

Live query results stream in after the Indexed items have been ranked, so a strictly sorted list would jump under the user's cursor and Enter could run something they never saw. An Extension scores each Live query result from 0 to 1, the Daemon maps that onto nucleo's scale and applies Frecency and Learned choices to both alike, and Results that arrive late are inserted below the visible rows instead of above them. The one exception is the calculator's answer, which takes the top row whenever the input parses as math.
