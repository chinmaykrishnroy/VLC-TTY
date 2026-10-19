# Removing terminal borders on Windows

VLC-TTY fills the character grid reported by the terminal. It cannot paint the host's title bar, scrollbar, profile padding, or partial pixels outside that grid.

In Windows Terminal, open **Settings → Profiles → your profile → Appearance**. Set **Padding** to `0` and **Scrollbar visibility** to **Hidden**. These are the equivalent profile JSON properties:

```json
{
  "padding": "0",
  "scrollbarState": "hidden"
}
```

Apply these to the profile you use for playback, rather than replacing your whole settings file. Microsoft documents a default padding of 8 pixels on every side. The scrollbar and whole-cell rounding can make right/bottom gaps larger. A small remainder can still exist when a maximized window's pixel size is not an exact multiple of its font cells; adjust font size/window size if needed.

Press `f` inside VLC-TTY to hide the controls and request Windows Terminal fullscreen, including its tabs, title bar and the Windows taskbar. Or start with `vlc-tty --fullscreen movie.mkv`. The image is contained within the resulting terminal grid, and resizing invalidates the previous image. When OSD/captions are hidden, the canvas uses every available row and column; visible text temporarily reserves space so pixel graphics cannot cover it.

On local Windows Terminal sessions, the app identifies its own attached host through the pseudo-console owner and posts the default F11 action only to that terminal's focused input window. It checks monitor geometry to confirm the transition. Escape and quit restore a fullscreen transition initiated by VLC-TTY; a terminal that was already fullscreen is preserved. If the terminal handles F11 itself while the app owns fullscreen, the player restores its controls after detecting the host exit.

The bridge requires a foreground Windows Terminal with its default F11 fullscreen binding. It does not change profile settings or steal focus. Other terminals, customized bindings, unavailable host ownership and server-rendered SSH sessions retain player fullscreen and show a shortcut hint. Use your terminal's own action in those cases; Windows Terminal normally supports Alt+Enter. `stream` runs locally and can use the Windows bridge; a player running on the Pi cannot invoke Windows APIs on the SSH client. Native video-window mode is also outside this bridge.

Black bars beside square artwork or a video with a different aspect ratio are intentional: the image is fitted without cropping or stretching. They are separate from the outer terminal border. The album title/artist banner hides with the fullscreen OSD rather than permanently reserving three bottom rows.

Reference: [Microsoft Windows Terminal profile appearance](https://learn.microsoft.com/windows/terminal/customize-settings/profile-appearance).

Host implementation references: [Windows Terminal pseudo-console reparenting](https://github.com/microsoft/terminal/blob/main/src/cascadia/TerminalConnection/ConptyConnection.cpp), [fullscreen window management](https://github.com/microsoft/terminal/blob/main/src/cascadia/WindowsTerminal/IslandWindow.cpp). Geometry acknowledgement is necessary because posting the input message alone does not prove the host changed state.

## Album artwork and pixel graphics

Automatic mode queries the active terminal for image support. Recent Windows Terminal versions advertise Sixel, so audio covers and video use pixel graphics; the footer should say **Sixel pixel graphics**. Unsupported terminals still use **ANSI half-block fallback**. A startup probe times out after 750 ms without leaving a background reader or changing console input mode.

If you know your terminal supports Sixel but its capability replies are blocked, launch with `--graphics sixel`. Press `g` to toggle between the selected pixel renderer and ANSI. Sixel uses a colour palette and is much sharper than character-cell artwork; it is not a 24-bit image protocol.

The Windows CI smoke test drops a generated audio file with cover art into a real ConPTY session and requires Sixel output. It also tests fullscreen startup, wide/tall resizes, image containment, Escape, negative capability replies, unanswered queries, early keyboard input and normal exit. The headless harness has no Windows Terminal window, so it verifies the fallback and grid behavior; the native host transition requires an interactive Windows Terminal check. Its checksum-pinned Microsoft ConPTY test runtime is downloaded only for testing and is not bundled with VLC-TTY.

## October 19, 2026 fullscreen verification

On Windows x64, 108 deterministic Rust tests and strict Clippy passed. The real ConPTY test started with `--fullscreen`, dropped audio with generated cover art, and required contained Sixel rasters at 100×32, 160×24 and 60×40 cells, followed by Escape, clean quit and alternate-screen restoration. It initially reproduced the permanent three-row artwork banner; the corrected fullscreen path passes without that reservation. The existing ordinary artwork smoke also passed.

Separate owned test windows in installed Windows Terminal **1.24.12741.0** exercised the production host controller. DWM/monitor geometry reported windowed → fullscreen → windowed on quit; a window launched already fullscreen stayed fullscreen on player exit. Another launch could not post the host action and correctly reported the shortcut fallback. This confirms the native path works on this machine, while retaining the focus/binding limitations above. It does not establish other terminals' host support or movie-length rendering performance.
