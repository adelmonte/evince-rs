PREFIX ?= /usr
BINDIR ?= $(PREFIX)/bin
LIBEXECDIR ?= $(PREFIX)/lib
DATADIR ?= $(PREFIX)/share
SYSTEMDUSERDIR ?= $(PREFIX)/lib/systemd/user
DESTDIR ?=
CARGO ?= cargo
TARGET_DIR ?= target
CARGO_FLAGS ?=

all:
	$(CARGO) build --release --locked $(CARGO_FLAGS) --target-dir $(TARGET_DIR)

check:
	$(CARGO) test --release --locked $(CARGO_FLAGS) --target-dir $(TARGET_DIR)

install:
	install -Dm755 $(TARGET_DIR)/release/evince-rs $(DESTDIR)$(BINDIR)/evince-rs
	install -Dm755 $(TARGET_DIR)/release/evince-rs-previewer $(DESTDIR)$(BINDIR)/evince-rs-previewer
	install -Dm755 $(TARGET_DIR)/release/evince-rs-thumbnailer $(DESTDIR)$(BINDIR)/evince-rs-thumbnailer
	install -Dm755 $(TARGET_DIR)/release/evince-rs-daemon $(DESTDIR)$(LIBEXECDIR)/evince-rs-daemon
	cd data/icons && find . -type f -exec install -Dm644 {} $(DESTDIR)$(DATADIR)/evince-rs/icons/{} \;
	cd data/app-icons && find . -type f -exec install -Dm644 {} $(DESTDIR)$(DATADIR)/icons/hicolor/{} \;
	install -Dm644 data/io.github.adelmonte.EvinceRs.desktop $(DESTDIR)$(DATADIR)/applications/io.github.adelmonte.EvinceRs.desktop
	install -Dm644 data/io.github.adelmonte.EvinceRs.Previewer.desktop $(DESTDIR)$(DATADIR)/applications/io.github.adelmonte.EvinceRs.Previewer.desktop
	install -Dm644 data/io.github.adelmonte.EvinceRs.gschema.xml $(DESTDIR)$(DATADIR)/glib-2.0/schemas/io.github.adelmonte.EvinceRs.gschema.xml
	install -Dm644 data/evince-rs.thumbnailer $(DESTDIR)$(DATADIR)/thumbnailers/evince-rs.thumbnailer
	install -Dm644 data/io.github.adelmonte.EvinceRs.metainfo.xml $(DESTDIR)$(DATADIR)/metainfo/io.github.adelmonte.EvinceRs.metainfo.xml
	install -dm755 $(DESTDIR)$(DATADIR)/dbus-1/services $(DESTDIR)$(SYSTEMDUSERDIR)
	sed 's|@libexecdir@|$(LIBEXECDIR)|' data/io.github.adelmonte.EvinceRs.Daemon.service.in > $(DESTDIR)$(DATADIR)/dbus-1/services/io.github.adelmonte.EvinceRs.Daemon.service
	sed 's|@libexecdir@|$(LIBEXECDIR)|' data/io.github.adelmonte.EvinceRs.service.in > $(DESTDIR)$(SYSTEMDUSERDIR)/io.github.adelmonte.EvinceRs.service
	for m in evince-rs evince-rs-previewer evince-rs-thumbnailer; do install -Dm644 data/$$m.1 $(DESTDIR)$(DATADIR)/man/man1/$$m.1; done

.PHONY: all check install
