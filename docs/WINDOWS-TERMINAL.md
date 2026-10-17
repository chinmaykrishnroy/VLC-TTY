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

Use the terminal's fullscreen action to hide window chrome. `f`/F11 inside VLC-TTY toggles its player layout; the terminal may intercept its own fullscreen shortcut first. The app's fullscreen canvas uses every available row and column when OSD/captions are hidden. Visible text temporarily reserves space so pixel graphics cannot cover it.

Black bars beside square artwork or a video with a different aspect ratio are intentional: the image is fitted without cropping or stretching. They are separate from the outer terminal border.

Reference: [Microsoft Windows Terminal profile appearance](https://learn.microsoft.com/windows/terminal/customize-settings/profile-appearance).
