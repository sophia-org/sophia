PREFIX ?= /usr/local
LIBDIR ?= $(PREFIX)/lib
INCLUDEDIR ?= $(PREFIX)/include/sophia-desktop
BUILD ?= build
VERSION = 0.5.1
CC ?= cc
AR ?= ar
CFLAGS ?= -O2 -g
override CFLAGS += -std=c99 -Wall -Wextra -Werror -pedantic
CPPFLAGS += -Isrc

NINE_P = $(sort $(wildcard src/nine_p/*.c))
FILES = $(sort $(wildcard src/shell_files/*.c src/shell_session/*.c src/native_session/*.c src/wm_files/*.c src/wm_session/*.c src/output_files/*.c src/output_session/*.c)) src/desktop_connection.c
objects = $(patsubst %.c,$(BUILD)/%.o,$(1))
LIBRARIES = $(BUILD)/libsophia-9p.a $(BUILD)/libsophia-desktop.a
PACKAGES = sophia-9p sophia-desktop

.PHONY: all check check-files check-spec check-generator install clean
all: $(LIBRARIES) $(addprefix $(BUILD)/,$(addsuffix .pc,$(PACKAGES)))

$(BUILD)/src/%.o: src/%.c
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) -MMD -MP -c $< -o $@

$(BUILD)/libsophia-9p.a: $(call objects,$(NINE_P))
	$(AR) rcs $@ $^
$(BUILD)/libsophia-desktop.a: $(call objects,$(FILES))
	$(AR) rcs $@ $^
$(BUILD)/%.pc: pkgconfig/%.pc.in GNUmakefile
	@mkdir -p $(@D)
	sed -e 's|@PREFIX@|$(PREFIX)|g' -e 's|@LIBDIR@|$(LIBDIR)|g' -e 's|@INCLUDEDIR@|$(INCLUDEDIR)|g' -e 's|@VERSION@|$(VERSION)|g' $< > $@

FILE_TESTS = sophia_9p_client_test sophia_shell_files_test sophia_shell_files_roles_test sophia_shell_files_descriptors_test sophia_shell_files_descriptor_candidates_test sophia_shell_files_descriptor_objects_test sophia_shell_files_staging_test desktop_connection_test shell_session_custody_test shell_session_events_test shell_session_descriptors_test shell_session_staging_test native_session_test wm_files_test wm_session_test output_files_test output_session_test
ifeq ($(shell uname -s),Linux)
$(BUILD)/desktop_connection_test: WRAPS = -Wl,--wrap=getsockopt
endif
$(addprefix $(BUILD)/,$(FILE_TESTS)): $(BUILD)/%: src/tests/%.c $(wildcard src/tests/*.h) $(BUILD)/libsophia-desktop.a $(BUILD)/libsophia-9p.a
	$(CC) $(CPPFLAGS) $(CFLAGS) -UNDEBUG $< -L$(BUILD) -lsophia-desktop -lsophia-9p $(WRAPS) $(LDFLAGS) $(LDLIBS) -o $@
check-files: $(addprefix $(BUILD)/,$(FILE_TESTS))
	@set -e; for test in $^; do "$$test"; done

check-spec:
	sha256sum --check spec/SHA256SUMS
# Needs python3; not part of check because Python is not a build dependency.
check-generator:
	python3 -B tools/generate_wm_rows.py --check
	cd tools && python3 -B test_generate_wm_rows.py
check: check-spec check-files

install: all
	install -d $(DESTDIR)$(LIBDIR) $(DESTDIR)$(LIBDIR)/pkgconfig $(DESTDIR)$(INCLUDEDIR)
	install -m 644 $(LIBRARIES) $(DESTDIR)$(LIBDIR)/
	install -m 644 src/*.h $(DESTDIR)$(INCLUDEDIR)/
	install -m 644 $(addprefix $(BUILD)/,$(addsuffix .pc,$(PACKAGES))) $(DESTDIR)$(LIBDIR)/pkgconfig/
clean:
	rm -rf $(BUILD)

-include $(patsubst %.o,%.d,$(call objects,$(NINE_P) $(FILES)))
