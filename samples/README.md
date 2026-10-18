# Test media

`tagged-audio.mp3` is a synthetic ten-second 440 Hz tone at 32 kbit/s, with a generated 64 × 64 blue PNG embedded as ID3v2.3 cover art. Its title, artist and album are `SSH Metadata Track`, `VLC-TTY Test Artist` and `Stream Regression Album`, encoded as UTF-16LE with a byte-order mark.

The fixture was generated with FFmpeg's `sine` source and `libmp3lame`, then remuxed with the PNG using `-c copy -disposition:v attached_pic -id3v2_version 3`. The three text frames were converted to UTF-16LE and their ID3 frame/tag lengths updated. It contains no third-party music or artwork. Native tests serve it over HTTP and verify that tags, decoded artwork and the playback clock reach the app, including when the URL contains spaces and a query string.
