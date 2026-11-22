# evince-rs

A document viewer for PDF, PostScript, DjVu, TIFF, XPS, DVI and comic book archives.
It is a Rust and GTK 3 port of [Evince](https://gitlab.gnome.org/GNOME/evince).

## Install

Arch Linux (AUR):

```bash
yay -S evince-rs
```

From source:

```bash
make
sudo make install          # PREFIX=/usr by default
```

The PostScript, DjVu and XPS backends need libspectre, djvulibre and libgxps.
`make CARGO_FLAGS=--no-default-features` leaves them out; the AUR package is built that way.

## What's included

| Program | Does |
|---|---|
| `evince-rs` | The viewer |
| `evince-rs-previewer` | Print preview for PostScript and PDF |
| `evince-rs-thumbnailer` | PNG thumbnails for file managers |
| `evince-rs-daemon` | D-Bus service that tracks open documents (started on demand) |

Each has a man page. Settings live in GSettings under `io.github.adelmonte.EvinceRs`.

## Dependencies

gtk3, poppler-glib, libarchive, libtiff, libsynctex, cairo, pango, gdk-pixbuf2 and
gsettings-desktop-schemas. Optional: gvfs (session saving and bookmarks), texlive-bin
(DVI), bzip2 and xz (compressed documents).

## Tests

```bash
make check
```

## License

GPL-2.0-or-later, the same as Evince. See [COPYING](COPYING).
