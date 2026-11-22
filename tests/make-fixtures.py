#!/usr/bin/env python3
"""Generates the test documents used by tests/backends.rs."""
import cairo, os, subprocess, sys, zipfile, gzip, shutil, tempfile

out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(out, exist_ok=True)

def draw_pages(surface, n, w, h, links=False):
    cr = cairo.Context(surface)
    for i in range(n):
        cr.set_source_rgb(1, 1, 1)
        cr.paint()
        cr.set_source_rgb(0, 0, 0)
        cr.select_font_face("Sans")
        cr.set_font_size(24)
        cr.move_to(72, 100)
        cr.show_text(f"Hello World page {i + 1}")
        cr.move_to(72, 140)
        cr.show_text("findme" if i == 1 else "nothing here")
        if links:
            cr.tag_begin(cairo.TAG_DEST, f"name='p{i+1}'")
            cr.tag_end(cairo.TAG_DEST)
            cr.move_to(72, 200)
            cr.tag_begin(cairo.TAG_LINK, "uri='https://example.org/'")
            cr.show_text("external link")
            cr.tag_end(cairo.TAG_LINK)
            cr.move_to(72, 240)
            cr.tag_begin(cairo.TAG_LINK, "dest='p3'")
            cr.show_text("go to page three")
            cr.tag_end(cairo.TAG_LINK)
        cr.rectangle(72, 300, 200, 100)
        cr.set_source_rgb(0.2, 0.4, 0.8)
        cr.fill()
        cr.show_page()

pdf = cairo.PDFSurface(os.path.join(out, "sample.pdf"), 612, 792)
pdf.set_metadata(cairo.PDFMetadata.TITLE, "Sample Title")
pdf.set_metadata(cairo.PDFMetadata.AUTHOR, "Test Author")
pdf.set_page_label("i")
root = pdf.add_outline(cairo.PDF_OUTLINE_ROOT, "Chapter 1", "page=1", cairo.PDFOutlineFlags.OPEN)
pdf.add_outline(root, "Section 1.1", "page=2", 0)
pdf.add_outline(cairo.PDF_OUTLINE_ROOT, "Chapter 2", "page=3", 0)
draw_pages(pdf, 3, 612, 792, links=True)
pdf.finish()

with open(os.path.join(out, "sample.pdf"), "rb") as f, gzip.open(os.path.join(out, "sample.pdf.gz"), "wb") as g:
    shutil.copyfileobj(f, g)

ps = cairo.PSSurface(os.path.join(out, "sample.ps"), 612, 792)
draw_pages(ps, 2, 612, 792)
ps.finish()

tmp = tempfile.mkdtemp()
pngs = []
for i in range(3):
    s = cairo.ImageSurface(cairo.FORMAT_RGB24, 300 + 50 * i, 400)
    draw_pages(s, 1, 300, 400)
    p = os.path.join(tmp, f"page{i + 1:02d}.png")
    s.write_to_png(p)
    pngs.append(p)

with zipfile.ZipFile(os.path.join(out, "sample.cbz"), "w") as z:
    for p in reversed(pngs):
        z.write(p, os.path.basename(p))
    z.writestr("._page00.png", b"junk")
    z.writestr("notes.txt", b"not an image")

subprocess.check_call(["magick", *pngs[:2], "-density", "72", "-units", "PixelsPerInch",
                       os.path.join(out, "sample.tiff")])

djvus = []
for i, p in enumerate(pngs[:2]):
    ppm = os.path.join(tmp, f"p{i}.ppm")
    subprocess.check_call(["magick", p, "-density", "100", ppm])
    d = os.path.join(tmp, f"p{i}.djvu")
    subprocess.check_call(["c44", "-dpi", "100", ppm, d])
    djvus.append(d)
subprocess.check_call(["djvm", "-c", os.path.join(out, "sample.djvu"), *djvus])
shutil.rmtree(tmp)
print("fixtures written to", out)
