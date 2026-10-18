# Streamed music metadata

`vlc-tty stream HOST /path/to/song.mp3` plays original compressed bytes through an SSH tunnel. LibVLC discovers title, artist and album during playback; the UI polls those fields independently of playback-state changes, so tags can arrive after playback starts.

The title fallback decodes the URL's final filename component once. It does not change the actual URL, treat `+` as a space, expose query parameters as a title, or decode percent signs in local filenames.

Cached `file://` artwork reported by LibVLC is decoded on a background worker. Some LibVLC 3 HTTP MP3 paths report text tags without exposing the embedded picture. For those, the worker requests the first ten bytes, validates the ID3 header, and requests only the declared tag. Both requests require exact HTTP range responses. Tags are capped at 16 MiB, each request has a three-second timeout, and redirects/proxies are disabled. This fallback uses HTTP because the SSH tunnel's local endpoint is HTTP; it does not add general HTTPS artwork fetching or online album searches.

The same ID3 block supplies text tags when LibVLC omits them for an HTTP stream. Provisional URL titles cannot overwrite a parsed song title. One pending artwork job is replaceable, and results carry the media revision so old covers or tags cannot appear on a new track. Covers are decoded with image dimension and allocation limits. Network tag reads are attempted once per track; a failed cover fetch leaves playback running. Separate remote folder images are not currently served by the single-file tunnel.

The synthetic fixture and generation notes are in [samples](../samples/README.md). The native HTTP regression requires the title, artist, album, 64 × 64 embedded cover and an advancing playback clock to reach the app through its normal update loop.
