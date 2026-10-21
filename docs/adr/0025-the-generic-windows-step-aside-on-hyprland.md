# 25. The generic windows step aside on Hyprland

Status: accepted (2026-10-21)

The system Extension lists the open windows of any compositor with wlr-foreign-toplevel-management. Hyprland has that protocol too, and its own Extension lists the same windows from its event socket, with Actions no Wayland protocol offers ("Move to Workspace", "Toggle Floating"). With both running, every window on Hyprland would show twice.

The system Extension now leaves the windows to the Hyprland Extension on Hyprland. When it connects to the compositor, it reads the globals the compositor advertises. Any global whose interface starts with `hyprland_` (Hyprland advertises several, such as `hyprland_toplevel_export_manager_v1`) means the compositor is Hyprland: the system Extension binds nothing, logs that it leaves the windows to the Hyprland Extension, and its windows part ends. It sends no windows, takes no events and uses no CPU. Its power Items are not affected.

Reading the globals happens on the connection the windows would use, so it can't be fooled by a stale `HYPRLAND_INSTANCE_SIGNATURE` or by Hyprland starting after the Extension. It costs nothing more: the registry is read before binding anyway.

An Extension for another compositor, sway or niri say, that lists windows would be added to the same check, by a global or a socket only that compositor has.

Each Extension's windows keep their own `address`, the field of the shared kind `window`: Hyprland's is its window address (`0x55d0c6a1b2c0`), the system Extension's is its own number for the toplevel (`window-3`), since the protocol gives a toplevel no name. On any one compositor only one of them lists windows, so a Contributed action on `window` meets one kind of address there.

## Considered options

- **Both Extensions give the window the same Identity, so it shows once.** Rejected: an Identity shares History, it doesn't merge Results (ADR 0008), so the window would still show twice. Merging would need a new rule in the Daemon. And there is nothing to name the window by: a wlr-foreign-toplevel handle carries no id at all, and Hyprland's window address is not in the protocol, so the two Extensions can't spell one window the same way.
- **The Daemon hides the generic windows while another Extension sends Items of kind `window`.** Rejected: the Daemon would have to know that one Extension's windows stand for another's, which is a rule about two Extensions written into the Daemon. It would also index both lists for nothing.
- **A setting that turns the generic windows off.** Rejected as the answer: every Hyprland user would have to find it before the menu stops showing windows twice. It may still come, for someone who removes the Hyprland Extension and wants the generic windows back.
- **Detect Hyprland from `HYPRLAND_INSTANCE_SIGNATURE`.** Rejected: the Daemon, and so the Extension, may start before Hyprland hands that variable to the user manager, and a variable left from an earlier session would make the windows step aside on another compositor.
