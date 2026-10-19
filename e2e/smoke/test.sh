# shellcheck shell=bash
# Quickshell shows a layer-shell window and it reaches the screen as drawn.
spawn quickshell --path shell.qml
expect_screen reference.png
