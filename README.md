
# Phill's Custom Build of DWM #

This is a Rust port of suckless dwm + dmenu, ported from X11 to Wayland.
The compositor is built on the [smithay](https://crates.io/crates/smithay)
framework and runs natively as a Wayland compositor (no X server required).

**Linux-only.** The compositor depends on `libudev`, `libdrm`, `libgbm`,
`libinput`, `libseat`, `libxkbcommon` and a working DRM/KMS stack; it cannot
be built or run on macOS or Windows. The menu (dmenu) only requires a
Wayland session that supports `wlr-layer-shell-unstable-v1`.

dwm is a dynamic window manager. It manages windows in tiled, monocle and floating layouts. All of the layouts can be applied dynamically, optimising the environment for the application in use and the task performed.

In tiled layout windows are managed in a master and stacking area. The master area contains the window which currently needs most attention, whereas the stacking area contains all other windows. In monocle layout all windows are maximised to the screen size. In floating layout windows can be resized and moved freely. Dialog windows are always managed floating, regardless of the layout applied.

Windows are grouped by tags. Each window can be tagged with one or multiple tags. Selecting certain tags displays all windows with these tags.

Each screen contains a small status bar which displays all available tags, the layout, the number of visible windows, the title of the focused window, and the text read from the root window name property, if the screen is focused. A floating window is indicated with an empty square and a maximised floating window is indicated with a filled square before the windows title. The selected tags are indicated with a different color. The tags of the focused window are indicated with a filled square in the top left corner. The tags which are applied to one or more windows are indicated with an empty square in the top left corner.

dwm draws a small customizable border around windows to indicate the focus state.

![Screenshot](http://dwm.suckless.org/screenshots/dwm-20100318.png)

My version differs by:

* being patched to remember where the split is on a per-tag basis
* thick yellow borders
* gnome-terminal as the terminal emulator
